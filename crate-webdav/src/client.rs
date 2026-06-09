use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use url::Url;

use crate::error::{Result, WebDavError};
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

        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", username.as_ref(), password.as_ref()));
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
        let response = self
            .client
            .get(url)
            .headers(self.headers())
            .send()
            .await?;

        self.handle_fetch_response(response).await
    }

    pub async fn read_file_to_string(&self, path: &str) -> Result<String> {
        let bytes = self.read_file(path).await?;
        String::from_utf8(bytes)
            .map_err(|e| WebDavError::XmlParse(format!("Invalid UTF-8: {e}")))
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

        self.handle_status_discard(response, |s| s == 201 || s == 204 || s == 200).await
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

        self.handle_status(response, |s| s == 204 || s == 200 || s == 404).await?;
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

        self.handle_status_discard(response, |s| s == 201 || s == 204 || s == 200).await
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

    async fn handle_fetch_response(
        &self,
        response: reqwest::Response,
    ) -> Result<Vec<u8>> {
        match response.status().as_u16() {
            200 | 207 => Ok(response.bytes().await?.to_vec()),
            401 => Err(WebDavError::AuthFailed("Invalid credentials".into())),
            404 => Err(WebDavError::NotFound(format!(
                "Path not found"
            ))),
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
            _ => Err(WebDavError::UnexpectedStatus {
                status,
                body,
            }),
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

            let name = href
                .rsplit('/')
                .next()
                .unwrap_or(&href)
                .to_string();

            if name.is_empty() {
                continue;
            }

            let prop = response
                .propstats
                .first()
                .map(|ps| &ps.prop);

            if let Some(prop) = prop {
                let is_dir = prop.resourcetype.collection.is_some();
                let size = prop.getcontentlength.unwrap_or(0);
                let modified = prop
                    .getlastmodified
                    .clone()
                    .unwrap_or_default();

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
