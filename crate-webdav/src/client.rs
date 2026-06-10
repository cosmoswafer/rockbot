use base64::Engine;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use url::Url;

use crate::error::{Result, WebDavError};
use crate::path::WebDavPath;
use crate::types::{MultiStatus, WebDavEntry};

const MULTI_STATUS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<d:propfind xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns" xmlns:nc="http://nextcloud.org/ns">
  <d:prop>
    <d:getlastmodified/>
    <d:getcontentlength/>
    <d:getcontenttype/>
    <d:resourcetype/>
    <d:getetag/>
  </d:prop>
</d:propfind>"#;

#[derive(Debug, Clone)]
pub struct WebDavClient {
    base_url: String,
    client: reqwest::Client,
    auth_header: String,
}

impl WebDavClient {
    pub fn new(
        url: impl Into<String>,
        username: impl AsRef<str>,
        password: impl AsRef<str>,
    ) -> Result<Self> {
        let url: String = url.into();
        let mut base = url.trim_end_matches('/').to_string();
        if !base.ends_with('/') {
            base.push('/');
        }

        let encoded = base64::engine::general_purpose::STANDARD.encode(format!(
            "{}:{}",
            username.as_ref(),
            password.as_ref()
        ));
        let auth_header = format!("Basic {encoded}");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            base_url: base,
            client,
            auth_header,
        })
    }

    fn full_url(&self, path: &str) -> Result<Url> {
        let path = path.trim_start_matches('/');
        Url::parse(&format!("{}{path}", self.base_url))
            .map_err(|e| WebDavError::InvalidUrl(format!("Failed to build URL: {e}")))
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&self.auth_header).unwrap(),
        );
        headers
    }

    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let url = self.full_url(path)?;
        let response = self.client.get(url).headers(self.headers()).send().await?;

        self.handle_fetch_response(response).await
    }

    pub async fn read_file_to_string(&self, path: &str) -> Result<String> {
        let bytes = self.read_file(path).await?;
        String::from_utf8(bytes).map_err(|e| WebDavError::XmlParse(format!("Invalid UTF-8: {e}")))
    }

    pub async fn write_file(&self, path: &str, content: impl Into<bytes::Bytes>) -> Result<()> {
        let url = self.full_url(path)?;
        let content = content.into();
        let response = self
            .client
            .put(url)
            .headers(self.headers())
            .body(content)
            .send()
            .await?;

        self.handle_status_discard(response, |s| s == 201 || s == 204 || s == 200)
            .await
    }

    pub async fn list_directory(&self, path: &str) -> Result<Vec<WebDavEntry>> {
        let url = self.full_url(path)?;
        let mut headers = self.headers();
        headers.insert("Depth", HeaderValue::from_static("1"));

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url)
            .headers(headers)
            .header("Content-Type", "application/xml")
            .body(MULTI_STATUS_XML)
            .send()
            .await?;

        let body = self
            .handle_status(response, |s| s == 207)
            .await?
            .unwrap_or_default();

        if body.is_empty() {
            return Ok(vec![]);
        }

        self.parse_propfind_response(&body)
    }

    pub async fn ensure_directory(&self, path: &str) -> Result<()> {
        let url = self.full_url(path)?;
        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), url)
            .headers(self.headers())
            .send()
            .await?;

        match response.status().as_u16() {
            201 => Ok(()),
            405 => Err(WebDavError::AlreadyExists(format!(
                "Directory already exists or path conflicts: {path}"
            ))),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(WebDavError::UnexpectedStatus { status, body })
            }
        }
    }

    pub async fn ensure_directory_all(&self, path: &str) -> Result<()> {
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let mut current = String::new();

        for part in parts {
            if part.is_empty() {
                continue;
            }
            current.push('/');
            current.push_str(part);

            match self.ensure_directory(&current).await {
                Ok(()) => {}
                Err(WebDavError::AlreadyExists(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        let url = self.full_url(path)?;
        let response = self
            .client
            .delete(url)
            .headers(self.headers())
            .send()
            .await?;

        self.handle_status(response, |s| s == 204 || s == 200 || s == 404)
            .await?;
        Ok(())
    }

    pub async fn exists(&self, path: &str) -> Result<bool> {
        let url = self.full_url(path)?;
        let mut headers = self.headers();
        headers.insert("Depth", HeaderValue::from_static("0"));

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url)
            .headers(headers)
            .header("Content-Type", "application/xml")
            .body(MULTI_STATUS_XML)
            .send()
            .await?;

        match response.status().as_u16() {
            207 => Ok(true),
            404 => Ok(false),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(WebDavError::UnexpectedStatus { status, body })
            }
        }
    }

    /// Write file with auto-creation of parent directories via X-NC-WebDAV-AutoMkcol
    pub async fn write_file_auto_mkcol(
        &self,
        path: &str,
        content: impl Into<bytes::Bytes>,
    ) -> Result<()> {
        let url = self.full_url(path)?;
        let content = content.into();
        let mut headers = self.headers();
        headers.insert("X-NC-WebDAV-AutoMkcol", HeaderValue::from_static("1"));

        let response = self
            .client
            .put(url)
            .headers(headers)
            .body(content)
            .send()
            .await?;

        self.handle_status_discard(response, |s| s == 201 || s == 204 || s == 200)
            .await
    }

    /// Write a file, falling back to explicit mkdir if AutoMkcol is not supported.
    ///
    /// First tries `write_file_auto_mkcol`. If the server returns 404 (path not
    /// found), the parent directory is explicitly created via `ensure_directory_all`,
    /// then a plain PUT is retried without the NextCloud-specific header.
    pub async fn write_file_with_fallback(
        &self,
        path: &str,
        content: impl Into<bytes::Bytes> + Clone,
    ) -> Result<()> {
        let content = content.into();
        match self.write_file_auto_mkcol(path, content.clone()).await {
            Ok(()) => return Ok(()),
            Err(WebDavError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }

        let parent = WebDavPath::parent_path(path);
        self.ensure_directory_all(&parent).await?;
        self.write_file(path, content).await
    }

    /// Ensure the root room directory exists for a given room_id.
    ///
    /// Creates `/{room_id}/` at the configured root if it doesn't already exist.
    /// Safe to call multiple times — uses `ensure_directory_all` which silently
    /// ignores already-existing path segments.
    pub async fn ensure_room_directory(&self, room_id: &str) -> Result<()> {
        let path = WebDavPath::new("").room_dir(room_id);
        self.ensure_directory_all(&path).await
    }

    async fn handle_status_discard<F: Fn(u16) -> bool>(
        &self,
        response: reqwest::Response,
        valid: F,
    ) -> Result<()> {
        let status = response.status().as_u16();
        if valid(status) {
            return Ok(());
        }

        let body = response.text().await.unwrap_or_default();
        match status {
            401 => Err(WebDavError::AuthFailed("Invalid credentials".into())),
            404 => Err(WebDavError::NotFound(body)),
            _ => Err(WebDavError::UnexpectedStatus { status, body }),
        }
    }

    async fn handle_fetch_response(&self, response: reqwest::Response) -> Result<Vec<u8>> {
        match response.status().as_u16() {
            200 | 207 => Ok(response.bytes().await?.to_vec()),
            401 => Err(WebDavError::AuthFailed("Invalid credentials".into())),
            404 => Err(WebDavError::NotFound("Path not found".to_string())),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(WebDavError::UnexpectedStatus { status, body })
            }
        }
    }

    async fn handle_status<F: Fn(u16) -> bool>(
        &self,
        response: reqwest::Response,
        valid: F,
    ) -> Result<Option<String>> {
        let status = response.status().as_u16();
        if valid(status) {
            let body = response.text().await.ok();
            return Ok(body);
        }

        let body = response.text().await.unwrap_or_default();
        match status {
            401 => Err(WebDavError::AuthFailed("Invalid credentials".into())),
            404 => Err(WebDavError::NotFound(body)),
            _ => Err(WebDavError::UnexpectedStatus { status, body }),
        }
    }

    fn parse_propfind_response(&self, xml: &str) -> Result<Vec<WebDavEntry>> {
        let multi_status: MultiStatus = quick_xml::de::from_str(xml)?;
        let mut entries = Vec::new();

        for response in &multi_status.responses {
            let href = response
                .href
                .trim_end_matches('/')
                .trim_start_matches('/')
                .to_string();

            let name = href.rsplit('/').next().unwrap_or(&href).to_string();

            if name.is_empty() {
                continue;
            }

            let prop = response.propstats.first().map(|ps| &ps.prop);

            if let Some(prop) = prop {
                let is_dir = prop.resourcetype.collection.is_some();
                let size = prop.getcontentlength.unwrap_or(0);
                let modified = prop.getlastmodified.clone().unwrap_or_default();

                entries.push(WebDavEntry {
                    name,
                    href,
                    is_dir,
                    size,
                    modified,
                });
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_client() -> WebDavClient {
        WebDavClient::new("https://example.com", "user", "pass").unwrap()
    }

    #[test]
    fn test_parse_propfind_empty() {
        let client = make_test_client();
        let xml = r#"<?xml version="1.0"?>
<multistatus />"#;
        let entries = client.parse_propfind_response(xml).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_propfind_single_entry_ns() {
        let client = make_test_client();
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/general/notes.txt</d:href>
    <d:propstat>
      <d:prop>
        <d:getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</d:getlastmodified>
        <d:getcontentlength>2048</d:getcontentlength>
        <d:resourcetype></d:resourcetype>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let entries = client.parse_propfind_response(xml).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "notes.txt");
        assert_eq!(entries[0].size, 2048);
    }

    #[test]
    fn test_parse_propfind_single_entry_no_ws() {
        let client = make_test_client();
        let xml = "<?xml version=\"1.0\"?><multistatus><response><href>/general/notes.txt</href><propstat><prop><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified><getcontentlength>2048</getcontentlength><resourcetype></resourcetype></prop></propstat></response></multistatus>";
        let entries = client.parse_propfind_response(xml).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "notes.txt");
        assert_eq!(entries[0].size, 2048);
    }

    #[test]
    fn test_parse_response_href_only() {
        #[derive(Debug, serde::Deserialize)]
        struct HrefOnly {
            href: String,
        }
        let xml = r#"<response><href>/general/notes.txt</href></response>"#;
        let resp: HrefOnly = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.href, "/general/notes.txt");
    }

    #[test]
    fn test_parse_response_two_strings() {
        #[derive(Debug, serde::Deserialize)]
        struct TwoStrings {
            href: String,
            status: String,
        }
        let xml = r#"<response><href>/general/notes.txt</href><status>HTTP/1.1 200 OK</status></response>"#;
        let resp: TwoStrings = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.href, "/general/notes.txt");
        assert_eq!(resp.status, "HTTP/1.1 200 OK");
    }

    #[test]
    fn test_parse_response_string_then_vec() {
        #[derive(Debug, serde::Deserialize)]
        struct StringThenVec {
            href: String,
            #[serde(rename = "item")]
            items: Vec<String>,
        }
        let xml =
            r#"<response><href>/general/notes.txt</href><item>a</item><item>b</item></response>"#;
        let resp: StringThenVec = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.href, "/general/notes.txt");
        assert_eq!(resp.items, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn test_parse_response_string_then_struct_vec() {
        #[derive(Debug, serde::Deserialize)]
        struct Inner {
            value: String,
        }
        #[derive(Debug, serde::Deserialize)]
        struct StringThenStructVec {
            href: String,
            #[serde(rename = "item")]
            items: Vec<Inner>,
        }
        let xml = r#"<response><href>/notes.txt</href><item><value>a</value></item><item><value>b</value></item></response>"#;
        let resp: StringThenStructVec = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.href, "/notes.txt");
        assert_eq!(resp.items[0].value, "a");
        assert_eq!(resp.items[1].value, "b");
    }

    #[test]
    fn test_parse_prop_minimal() {
        #[derive(Debug, serde::Deserialize)]
        struct MinProp {
            #[serde(rename = "getcontentlength", default)]
            getcontentlength: Option<u64>,
        }
        let xml = r#"<prop><getcontentlength>2048</getcontentlength></prop>"#;
        let p: MinProp = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getcontentlength, Some(2048));
    }

    #[test]
    fn test_parse_prop_with_resourcetype() {
        #[derive(Debug, serde::Deserialize, Default)]
        struct ResourceType {
            #[serde(rename = "collection", default)]
            collection: Option<Empty>,
        }
        #[derive(Debug, serde::Deserialize)]
        struct Empty {}
        #[derive(Debug, serde::Deserialize)]
        struct PropWithRT {
            #[serde(rename = "getcontentlength", default)]
            getcontentlength: Option<u64>,
            #[serde(rename = "resourcetype", default)]
            resourcetype: ResourceType,
        }
        let xml = r#"<prop><getcontentlength>2048</getcontentlength><resourcetype></resourcetype></prop>"#;
        let p: PropWithRT = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getcontentlength, Some(2048));
    }

    #[test]
    fn test_parse_prop_with_rt_collection() {
        #[derive(Debug, serde::Deserialize, Default)]
        struct ResourceType {
            #[serde(rename = "collection", default)]
            collection: Option<Empty>,
        }
        #[derive(Debug, serde::Deserialize)]
        struct Empty {}
        #[derive(Debug, serde::Deserialize)]
        struct PropWithRT {
            #[serde(rename = "getcontentlength", default)]
            getcontentlength: Option<u64>,
            #[serde(rename = "resourcetype", default)]
            resourcetype: ResourceType,
        }
        let xml = r#"<prop><getcontentlength>2048</getcontentlength><resourcetype><collection></collection></resourcetype></prop>"#;
        let p: PropWithRT = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getcontentlength, Some(2048));
    }

    #[test]
    fn test_parse_prop_with_opt_string() {
        #[derive(Debug, serde::Deserialize)]
        struct PropWithOpt {
            #[serde(rename = "getcontentlength", default)]
            getcontentlength: Option<u64>,
            #[serde(rename = "getlastmodified", default)]
            getlastmodified: Option<String>,
        }
        let xml = r#"<prop><getcontentlength>2048</getcontentlength><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified></prop>"#;
        let p: PropWithOpt = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getcontentlength, Some(2048));
        assert_eq!(
            p.getlastmodified.as_deref(),
            Some("Mon, 01 Jan 2024 00:00:00 GMT")
        );
    }

    #[test]
    fn test_parse_prop_all_fields() {
        use crate::types::Prop;
        let xml = r#"<prop><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified><getcontentlength>2048</getcontentlength><getcontenttype>text/plain</getcontenttype><resourcetype></resourcetype><getetag>"abc123"</getetag></prop>"#;
        let p: Prop = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getcontentlength, Some(2048));
        assert_eq!(p.getcontenttype.as_deref(), Some("text/plain"));
        assert_eq!(
            p.getlastmodified.as_deref(),
            Some("Mon, 01 Jan 2024 00:00:00 GMT")
        );
    }

    #[test]
    fn test_parse_prop_getetag_with_quote() {
        use crate::types::Prop;
        let xml = r#"<prop><getetag>"abc123"</getetag></prop>"#;
        let p: Prop = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getetag.as_deref(), Some("\"abc123\""));
    }

    #[test]
    fn test_parse_prop_directly() {
        use crate::types::Prop;
        let xml = r#"<prop><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified><getcontentlength>2048</getcontentlength><resourcetype></resourcetype></prop>"#;
        let p: Prop = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getcontentlength, Some(2048));
    }

    #[test]
    fn test_parse_propstat_directly() {
        use crate::types::PropStat;
        let xml = r#"<propstat><prop><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified><getcontentlength>2048</getcontentlength><resourcetype></resourcetype></prop></propstat>"#;
        let ps: PropStat = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(ps.prop.getcontentlength, Some(2048));
    }

    #[test]
    fn test_parse_response_directly() {
        let xml = r#"<response><href>/general/notes.txt</href><propstat><prop><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified><getcontentlength>2048</getcontentlength><resourcetype></resourcetype></prop></propstat></response>"#;
        let resp: crate::types::Response = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.href, "/general/notes.txt");
        assert_eq!(resp.propstats.len(), 1);
    }
}
