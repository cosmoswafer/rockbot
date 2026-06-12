use base64::Engine;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use tracing::{error, info};
use url::Url;

use crate::calendar::{
    CalDavMultiStatus, CALENDAR_QUERY_EVENT_XML, CALENDAR_QUERY_TODO_XML, parse_vevents,
    parse_vtodos,
};
use crate::error::{Result, WebDavError};
use crate::path::WebDavPath;
use crate::types::{CaldavEvent, CaldavTodo, MultiStatus, WebDavEntry};

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

    pub async fn create_nextcloud_share_link(&self, file_path: &str) -> Option<String> {
        let server_root = match url::Url::parse(&self.base_url) {
            Ok(parsed) => format!("{}://{}", parsed.scheme(), parsed.host_str()?),
            Err(_) => {
                // Fallback: extract scheme+host from string
                if let Some(pos) = self.base_url.find("://") {
                    let after_scheme = &self.base_url[pos + 3..];
                    if let Some(host_end) = after_scheme.find('/') {
                        format!("{}://{}", &self.base_url[..pos], &after_scheme[..host_end])
                    } else {
                        tracing::warn!("Cannot parse base_url for OCS endpoint");
                        return None;
                    }
                } else {
                    tracing::warn!("Cannot parse base_url for OCS endpoint");
                    return None;
                }
            }
        };
        let share_url = format!(
            "{}/ocs/v2.php/apps/files_sharing/api/v1/shares",
            server_root.trim_end_matches('/')
        );
        let ocs_path = {
            let root = self
                .base_url
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("");
            let clean = file_path.trim_start_matches('/');
            // If the path starts with the root, use it as-is (minus leading /)
            // Otherwise prepend the root (e.g. image_gen paths from WebDavPath::new(""))
            if clean.starts_with(root) || root.is_empty() {
                clean.replace("//", "/")
            } else {
                format!("{}/{}", root, clean).replace("//", "/")
            }
        };
        let expire_date = {
            let now = time::OffsetDateTime::now_utc();
            let seven_days = now + time::Duration::days(7);
            seven_days
                .date()
                .format(&time::format_description::parse("[year]-[month]-[day]").unwrap())
                .unwrap_or_default()
        };
        let body = format!(
            "path={}&shareType=3&permissions=1&expireDate={}",
            percent_encoding::percent_encode(
                ocs_path.as_bytes(),
                percent_encoding::NON_ALPHANUMERIC
            ),
            expire_date
        );

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&self.auth_header).unwrap());
        headers.insert("OCS-APIRequest", HeaderValue::from_static("true"));
        headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );

        let resp = match self
            .client
            .post(&share_url)
            .headers(headers)
            .body(body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("NextCloud share link creation failed (HTTP): {}", e);
                return None;
            }
        };

        let resp_body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("NextCloud share link creation failed (body): {}", e);
                return None;
            }
        };

        // Extract share URL from OCS XML response: <url>https://...</url>
        let tag = "<url>";
        if let Some(start) = resp_body.find(tag) {
            let start = start + tag.len();
            if let Some(end) = resp_body[start..].find("</url>") {
                let raw = &resp_body[start..start + end];
                // OCS returns HTML-encoded angle brackets; fix common artifacts
                let cleaned = raw
                    .replace("&amp;", "&")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">");
                tracing::debug!(
                    "Created NextCloud share link for '{}': {}",
                    file_path,
                    cleaned
                );
                return Some(cleaned);
            }
        }

        tracing::warn!(
            "NextCloud share link creation failed (no <url> in response): {}",
            &resp_body[..resp_body.len().min(200)]
        );
        None
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
        info!("WebDAV PROPFIND: {}", url);
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

        let status = response.status().as_u16();
        info!("WebDAV PROPFIND response: status={}", status);

        let body = self
            .handle_status(response, |s| s == 207)
            .await?
            .unwrap_or_default();

        if body.is_empty() {
            return Ok(vec![]);
        }

        info!(
            "WebDAV PROPFIND body ({} chars): {}",
            body.len(),
            &body[..body.len().min(500)]
        );

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

    pub async fn ensure_room_directory(&self, room_id: &str) -> Result<()> {
        let path = WebDavPath::new("").room_dir(room_id);
        self.ensure_directory_all(&path).await
    }

    // ── CalDAV Calendar Operations ──────────────────────────────────────────────

    pub async fn list_events_by_date_range(
        &self,
        caldav_base_url: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<CaldavEvent>> {
        let base = caldav_base_url.trim_end_matches('/');
        let url = format!("{}/", base);
        let body = CALENDAR_QUERY_EVENT_XML
            .replace("START_PLACEHOLDER", start)
            .replace("END_PLACEHOLDER", end);

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), &url)
            .headers(self.headers())
            .header("Content-Type", "application/xml")
            .header("Depth", "1")
            .body(body)
            .send()
            .await?;

        let _status = response.status().as_u16();
        let xml = self
            .handle_status(response, |s| s == 207)
            .await?
            .unwrap_or_default();

        if xml.is_empty() {
            return Ok(vec![]);
        }

        let ms: CalDavMultiStatus =
            quick_xml::de::from_str(&xml).map_err(|e| WebDavError::XmlParse(e.to_string()))?;

        let mut events = Vec::new();
        for resp in &ms.responses {
            for ps in &resp.propstats {
                if let Some(ref ics) = ps.prop.calendar_data {
                    let etag = ps.prop.getetag.as_deref().unwrap_or("");
                    let parsed = parse_vevents(ics, &resp.href, etag);
                    events.extend(parsed);
                }
            }
        }
        Ok(events)
    }

    pub async fn get_event(&self, caldav_base_url: &str, uid: &str) -> Result<CaldavEvent> {
        let base = caldav_base_url.trim_end_matches('/');
        let url = format!("{}/{}.ics", base, uid);
        let response = self.client.get(&url).headers(self.headers()).send().await?;

        let status = response.status().as_u16();
        if status == 404 {
            return Err(WebDavError::NotFound(format!("Event not found: {uid}")));
        }
        let ics = self
            .handle_status(response, |s| s == 200)
            .await?
            .unwrap_or_default();

        let events = parse_vevents(&ics, &format!("{}.ics", uid), "");
        events
            .into_iter()
            .next()
            .ok_or_else(|| WebDavError::XmlParse(format!("No VEVENT found in response for {uid}")))
    }

    pub async fn add_event(
        &self,
        caldav_base_url: &str,
        uid: &str,
        ics_body: &str,
    ) -> Result<()> {
        let base = caldav_base_url.trim_end_matches('/');
        let url = format!("{}/{}.ics", base, uid);

        let response = self
            .client
            .put(&url)
            .headers(self.headers())
            .header("Content-Type", "text/calendar; charset=utf-8")
            .body(ics_body.to_string())
            .send()
            .await?;

        self.handle_status_discard(response, |s| s == 201 || s == 204)
            .await
    }

    pub async fn update_event(
        &self,
        caldav_base_url: &str,
        uid: &str,
        ics_body: &str,
        etag: &str,
    ) -> Result<()> {
        let base = caldav_base_url.trim_end_matches('/');
        let url = format!("{}/{}.ics", base, uid);
        let mut headers = self.headers();
        headers.insert(
            "If-Match",
            reqwest::header::HeaderValue::from_str(etag).unwrap(),
        );

        let response = self
            .client
            .put(&url)
            .headers(headers)
            .header("Content-Type", "text/calendar; charset=utf-8")
            .body(ics_body.to_string())
            .send()
            .await?;

        self.handle_status_discard(response, |s| s == 200 || s == 204)
            .await
    }

    pub async fn delete_event(&self, caldav_base_url: &str, uid: &str) -> Result<()> {
        let base = caldav_base_url.trim_end_matches('/');
        let url = format!("{}/{}.ics", base, uid);
        let response = self
            .client
            .delete(&url)
            .headers(self.headers())
            .send()
            .await?;

        self.handle_status_discard(response, |s| s == 204 || s == 404)
            .await
    }

    pub async fn fetch_event_by_uid(
        &self,
        caldav_base_url: &str,
        uid: &str,
    ) -> Result<Option<CaldavEvent>> {
        let base = caldav_base_url.trim_end_matches('/');
        let url = format!("{}/{}.ics", base, uid);
        let response = self.client.get(&url).headers(self.headers()).send().await?;

        let status = response.status().as_u16();
        if status == 404 {
            return Ok(None);
        }
        let ics = self
            .handle_status(response, |s| s == 200)
            .await?
            .unwrap_or_default();

        let events = parse_vevents(&ics, &format!("{}.ics", uid), "");
        Ok(events.into_iter().next())
    }

    pub async fn calendar_exists(&self, caldav_url: &str) -> Result<bool> {
        let url = {
            let clean = caldav_url.trim_end_matches('/');
            format!("{}/", clean)
        };
        let mut headers = self.headers();
        headers.insert("Depth", HeaderValue::from_static("0"));

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
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

    pub async fn ensure_calendar(
        &self,
        caldav_url: &str,
        display_name: &str,
    ) -> Result<()> {
        let url = {
            let clean = caldav_url.trim_end_matches('/');
            format!("{}/", clean)
        };

        if self.calendar_exists(&url).await? {
            return Ok(());
        }

        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<C:mkcalendar xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:set>
    <D:prop>
      <D:displayname>{}</D:displayname>
      <C:supported-calendar-component-set>
        <C:comp name="VEVENT"/>
      </C:supported-calendar-component-set>
    </D:prop>
  </D:set>
</C:mkcalendar>"#,
            quick_xml::escape::escape(display_name)
        );

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"MKCALENDAR").unwrap(), &url)
            .headers(self.headers())
            .header("Content-Type", "application/xml")
            .body(body)
            .send()
            .await?;

        self.handle_status_discard(response, |s| s == 201 || s == 200)
            .await
    }

    pub async fn list_todos(
        &self,
        caldav_base_url: &str,
    ) -> Result<Vec<CaldavTodo>> {
        let base = caldav_base_url.trim_end_matches('/');
        let url = format!("{}/", base);

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), &url)
            .headers(self.headers())
            .header("Content-Type", "application/xml")
            .header("Depth", "1")
            .body(CALENDAR_QUERY_TODO_XML)
            .send()
            .await?;

        let _status = response.status().as_u16();
        let xml = self
            .handle_status(response, |s| s == 207)
            .await?
            .unwrap_or_default();

        if xml.is_empty() {
            return Ok(vec![]);
        }

        let ms: CalDavMultiStatus =
            quick_xml::de::from_str(&xml).map_err(|e| WebDavError::XmlParse(e.to_string()))?;

        let mut todos = Vec::new();
        for resp in &ms.responses {
            for ps in &resp.propstats {
                if let Some(ref ics) = ps.prop.calendar_data {
                    todos.extend(parse_vtodos(ics, &resp.href));
                }
            }
        }
        Ok(todos)
    }

    // ── Internal Helpers ────────────────────────────────────────────────────────

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
            400 => Err(WebDavError::BadRequest(format!("Invalid request data: {}", body))),
            401 => Err(WebDavError::AuthFailed("Invalid credentials".into())),
            404 => Err(WebDavError::NotFound(body)),
            _ => Err(WebDavError::UnexpectedStatus { status, body }),
        }
    }

    async fn handle_fetch_response(&self, response: reqwest::Response) -> Result<Vec<u8>> {
        match response.status().as_u16() {
            200 | 207 => Ok(response.bytes().await?.to_vec()),
            400 => Err(WebDavError::BadRequest("Invalid request data".into())),
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
            400 => Err(WebDavError::BadRequest(format!("Invalid request data: {}", body))),
            401 => Err(WebDavError::AuthFailed("Invalid credentials".into())),
            404 => Err(WebDavError::NotFound(body)),
            _ => Err(WebDavError::UnexpectedStatus { status, body }),
        }
    }

    fn parse_propfind_response(&self, xml: &str) -> Result<Vec<WebDavEntry>> {
        let multi_status: MultiStatus = quick_xml::de::from_str(xml)
            .inspect_err(|e| error!("PROPFIND XML parse error: {e}"))?;
        let mut entries = Vec::new();

        for response in &multi_status.responses {
            let href = response
                .href
                .trim_end_matches('/')
                .trim_start_matches('/')
                .to_string();

            let raw_name = href.rsplit('/').next().unwrap_or(&href);
            let name = percent_encoding::percent_decode_str(raw_name)
                .decode_utf8()
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| raw_name.to_string());

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
        assert!(p.resourcetype.collection.is_none());
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
        assert!(p.resourcetype.collection.is_some());
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

    #[test]
    fn test_parse_propstat_with_status_sibling() {
        let xml = r#"<propstat><prop><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified><getcontentlength>2048</getcontentlength><resourcetype></resourcetype></prop><status>HTTP/1.1 200 OK</status></propstat>"#;
        let ps: crate::types::PropStat = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(ps.prop.getcontentlength, Some(2048));
    }

    #[test]
    fn test_parse_propfind_full_nextcloud_style() {
        let client = make_test_client();
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns" xmlns:nc="http://nextcloud.org/ns">
  <d:response>
    <d:href>/remote.php/dav/files/user/rockbot/r-general/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/></d:resourcetype>
        <d:getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</d:getlastmodified>
        <d:getetag>"abc123"</d:getetag>
        <oc:permissions>RDNVW</oc:permissions>
        <oc:size>0</oc:size>
        <oc:favorite>0</oc:favorite>
        <nc:has-preview>false</nc:has-preview>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/files/user/rockbot/r-general/notes.txt</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype/>
        <d:getlastmodified>Mon, 02 Jan 2024 12:00:00 GMT</d:getlastmodified>
        <d:getcontentlength>4096</d:getcontentlength>
        <d:getcontenttype>text/plain</d:getcontenttype>
        <d:getetag>"def456"</d:getetag>
        <oc:permissions>RDNVW</oc:permissions>
        <oc:size>4096</oc:size>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:responsedescription>SabreDAV 1.8</d:responsedescription>
</d:multistatus>"#;
        let entries = client.parse_propfind_response(xml).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "r-general");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].name, "notes.txt");
        assert_eq!(entries[1].size, 4096);
        assert_eq!(
            entries[1].modified,
            "Mon, 02 Jan 2024 12:00:00 GMT"
        );
    }

    #[test]
    fn test_parse_prop_empty_getcontentlength() {
        let xml = r#"<prop><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified><getcontentlength/><resourcetype></resourcetype></prop>"#;
        let p: crate::types::Prop = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getcontentlength, None);
    }

#[test]
fn test_parse_prop_missing_getcontentlength() {
        let xml = r#"<prop><getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified><resourcetype></resourcetype></prop>"#;
        let p: crate::types::Prop = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(p.getcontentlength, None);
    }
}
