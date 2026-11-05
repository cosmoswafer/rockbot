use base64::Engine;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};
use url::Url;

use crate::calendar::{
    CalDavMultiStatus, CALENDAR_QUERY_EVENT_XML, CALENDAR_QUERY_TODO_XML, parse_vevents,
    parse_vtodos,
};
use crate::error::{Result, WebDavError};
use crate::path::WebDavPath;
use crate::types::{
    ByteWindow, CaldavEvent, CaldavTodo, FileStat, Line, LineIndex, LineWindow, MultiStatus,
    WebDavEntry,
};

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
            Ok(parsed) => {
                // Preserve explicit non-default ports (e.g. http://host:8080);
                // host_str() alone strips them and misroutes the OCS request.
                let port_suffix = match parsed.port() {
                    Some(p) => format!(":{p}"),
                    None => String::new(),
                };
                format!(
                    "{}://{}{port_suffix}",
                    parsed.scheme(),
                    parsed.host_str()?
                )
            }
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
                .format(&time::format_description::parse_borrowed::<1>("[year]-[month]-[day]").unwrap())
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
                // Append /download for direct raw file access. Verified in
                // production: /preview serves correct headers (image/png,
                // Content-Disposition: inline) but RocketChat fails to render
                // it inline (issue #97 revert), while /download renders fine.
                let download_url = format!("{}/download", cleaned.trim_end_matches('/'));
                tracing::debug!(
                    "Created NextCloud share link for '{}': {}",
                    file_path,
                    download_url
                );
                return Some(download_url);
            }
        }

        tracing::warn!(
            "NextCloud share link creation failed (no <url> in response): {}",
            truncate_debug(&resp_body, 200)
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
            truncate_debug(&body, 500)
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

    /// Move or rename a file/directory. In WebDAV, rename and move are the
    /// same operation — the `Destination` header specifies the target path.
    /// `Overwrite: F` prevents clobbering an existing target.
    pub async fn move_file(&self, from: &str, to: &str) -> Result<()> {
        let src_url = self.full_url(from)?;
        let dst_url = self.full_url(to)?;
        let mut headers = self.headers();
        headers.insert(
            "Destination",
            HeaderValue::from_str(dst_url.as_str())
                .map_err(|e| WebDavError::InvalidUrl(format!("Invalid destination URL: {e}")))?,
        );
        headers.insert("Overwrite", HeaderValue::from_static("F"));

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"MOVE").unwrap(), src_url)
            .headers(headers)
            .send()
            .await?;

        self.handle_status(response, |s| s == 201 || s == 204)
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

    // ── Large-file primitives ──────────────────────────────────────────────────

    /// Fetch depth-0 metadata for one path.
    pub async fn stat(&self, path: &str) -> Result<FileStat> {
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

        let xml = self
            .handle_status(response, |s| s == 207)
            .await?
            .ok_or_else(|| WebDavError::NotFound(path.to_string()))?;

        let multi_status: MultiStatus =
            quick_xml::de::from_str(&xml).map_err(|e| WebDavError::XmlParse(e.to_string()))?;
        let prop = multi_status
            .responses
            .first()
            .and_then(|r| r.propstats.first())
            .map(|ps| &ps.prop)
            .ok_or_else(|| WebDavError::XmlParse("PROPFIND returned no properties".into()))?;

        Ok(FileStat {
            size: prop.getcontentlength.unwrap_or(0),
            etag: prop.getetag.clone(),
            content_type: prop.getcontenttype.clone(),
            last_modified: prop.getlastmodified.clone(),
            is_dir: prop.resourcetype.collection.is_some(),
        })
    }

    /// Read a bounded byte range. Falls back to slicing a 200 response when the
    /// server ignores the Range header.
    pub async fn read_range(&self, path: &str, start: u64, len: u64) -> Result<ByteWindow> {
        if len == 0 {
            return Ok(ByteWindow {
                bytes: Vec::new(),
                start,
                total: None,
            });
        }
        let url = self.full_url(path)?;
        let end = start.saturating_add(len.saturating_sub(1));
        let response = self
            .client
            .get(url)
            .headers(self.headers())
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
            .send()
            .await?;

        let status = response.status().as_u16();
        let total = content_range_total(response.headers());

        match status {
            206 => Ok(ByteWindow {
                bytes: response.bytes().await?.to_vec(),
                start,
                total,
            }),
            200 => {
                // Range ignored: slice the full body locally.
                let body = response.bytes().await?;
                let from = (start as usize).min(body.len());
                let to = start.saturating_add(len).min(body.len() as u64) as usize;
                Ok(ByteWindow {
                    bytes: body[from..to].to_vec(),
                    start,
                    total: Some(body.len() as u64),
                })
            }
            401 => Err(WebDavError::AuthFailed("Invalid credentials".into())),
            404 => Err(WebDavError::NotFound(path.to_string())),
            416 => Err(WebDavError::RangeNotSatisfiable { start, len }),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(WebDavError::UnexpectedStatus { status, body })
            }
        }
    }

    /// Byte window plus the file size when the server did not report it.
    pub async fn read_byte_window(&self, path: &str, start: u64, len: u64) -> Result<ByteWindow> {
        let mut window = self.read_range(path, start, len).await?;
        if window.total.is_none() {
            window.total = self.stat(path).await.ok().map(|s| s.size);
        }
        Ok(window)
    }

    /// Read a line window from the start of the file.
    pub async fn read_window(
        &self,
        path: &str,
        start_line: u64,
        max_lines: usize,
        max_bytes: usize,
        max_line_chars: usize,
    ) -> Result<LineWindow> {
        self.read_window_from(path, 0, 1, start_line, max_lines, max_bytes, max_line_chars)
            .await
    }

    /// Read a line window beginning at a known byte offset (line-index
    /// checkpoint). `first_line_at_byte` is the 1-based line number at
    /// `byte_start`.
    #[allow(clippy::too_many_arguments)]
    pub async fn read_window_from(
        &self,
        path: &str,
        byte_start: u64,
        first_line_at_byte: u64,
        start_line: u64,
        max_lines: usize,
        max_bytes: usize,
        max_line_chars: usize,
    ) -> Result<LineWindow> {
        let start_line = start_line.max(1);
        let max_lines = max_lines.max(1);
        let max_bytes = max_bytes.max(1);
        let max_line_chars = max_line_chars.max(1);

        let url = self.full_url(path)?;
        let request = self.client.get(url).headers(self.headers());
        let request = if byte_start > 0 {
            request.header(reqwest::header::RANGE, format!("bytes={byte_start}-"))
        } else {
            request
        };
        let mut response = request.send().await?;

        let status = response.status().as_u16();
        let mut skip = 0u64;
        match status {
            200 => skip = byte_start, // Range ignored: discard the prefix.
            206 => {}
            401 => return Err(WebDavError::AuthFailed("Invalid credentials".into())),
            404 => return Err(WebDavError::NotFound(path.to_string())),
            416 => {
                return Err(WebDavError::RangeNotSatisfiable {
                    start: byte_start,
                    len: 0,
                });
            }
            status => {
                let body = response.text().await.unwrap_or_default();
                return Err(WebDavError::UnexpectedStatus { status, body });
            }
        }

        let mut buf: Vec<u8> = Vec::new();
        let mut line_no = first_line_at_byte.saturating_sub(1);
        let mut lines: Vec<Line> = Vec::new();
        let mut used = 0usize;
        let mut truncated = false;
        let mut next_line = start_line;

        'outer: while let Some(chunk) = response.chunk().await? {
            buf.extend_from_slice(&chunk);

            if skip > 0 {
                let drop = (skip as usize).min(buf.len());
                buf.drain(..drop);
                skip -= drop as u64;
                if skip > 0 {
                    continue;
                }
            }

            let mut consumed = 0usize;
            while let Some(rel) = buf[consumed..].iter().position(|&b| b == b'\n') {
                let end = consumed + rel;
                let raw = &buf[consumed..end];
                let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
                consumed = end + 1;
                line_no += 1;

                if line_no < start_line {
                    continue;
                }
                if lines.len() >= max_lines || used >= max_bytes {
                    truncated = true;
                    next_line = line_no;
                    break 'outer;
                }

                let text = String::from_utf8_lossy(raw);
                let text = truncate_line(&text, max_line_chars);
                used += text.len() + 1;
                lines.push(Line {
                    number: line_no,
                    text,
                });
                next_line = line_no + 1;
            }
            buf.drain(..consumed);

            // A file with no newlines (common for exported SVG) must not grow
            // the buffer without bound.
            if buf.len() > max_bytes.saturating_mul(8).max(1 << 20) {
                line_no += 1;
                let text = String::from_utf8_lossy(&buf);
                let text = truncate_line(&text, max_line_chars);
                if line_no >= start_line && lines.len() < max_lines {
                    lines.push(Line {
                        number: line_no,
                        text,
                    });
                    next_line = line_no + 1;
                }
                truncated = true;
                break;
            }
        }

        if !truncated && !buf.is_empty() {
            line_no += 1;
            if line_no >= start_line {
                if lines.len() >= max_lines || used >= max_bytes {
                    truncated = true;
                    next_line = line_no;
                } else {
                    let text = truncate_line(&String::from_utf8_lossy(&buf), max_line_chars);
                    lines.push(Line {
                        number: line_no,
                        text,
                    });
                    next_line = line_no + 1;
                }
            }
        }

        let reached_eof = !truncated;
        if lines.is_empty() {
            next_line = start_line;
        }

        Ok(LineWindow {
            lines,
            next_line,
            reached_eof,
            truncated,
        })
    }

    /// Stream every line without retaining the file. The callback returns
    /// `false` to stop early. Returns the number of lines visited.
    pub async fn for_each_line<F>(&self, path: &str, mut visit: F) -> Result<u64>
    where
        F: FnMut(u64, &str) -> bool,
    {
        let mut response = self
            .client
            .get(self.full_url(path)?)
            .headers(self.headers())
            .send()
            .await?;

        let status = response.status().as_u16();
        if status != 200 && status != 206 {
            let body = response.text().await.unwrap_or_default();
            return Err(match status {
                401 => WebDavError::AuthFailed("Invalid credentials".into()),
                404 => WebDavError::NotFound(path.to_string()),
                status => WebDavError::UnexpectedStatus { status, body },
            });
        }

        let mut buf: Vec<u8> = Vec::new();
        let mut line_no = 0u64;

        while let Some(chunk) = response.chunk().await? {
            buf.extend_from_slice(&chunk);
            let mut consumed = 0usize;
            while let Some(rel) = buf[consumed..].iter().position(|&b| b == b'\n') {
                let end = consumed + rel;
                let raw = &buf[consumed..end];
                let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
                consumed = end + 1;
                line_no += 1;
                if !visit(line_no, &String::from_utf8_lossy(raw)) {
                    return Ok(line_no);
                }
            }
            buf.drain(..consumed);
        }

        if !buf.is_empty() {
            line_no += 1;
            visit(line_no, &String::from_utf8_lossy(&buf));
        }
        Ok(line_no)
    }

    /// Build a sparse line index in one streaming pass.
    pub async fn build_line_index(&self, path: &str, checkpoint_every: u64) -> Result<LineIndex> {
        let stat = self.stat(path).await?;
        let every = checkpoint_every.max(1);

        let mut response = self
            .client
            .get(self.full_url(path)?)
            .headers(self.headers())
            .send()
            .await?;
        let status = response.status().as_u16();
        if status != 200 && status != 206 {
            let body = response.text().await.unwrap_or_default();
            return Err(WebDavError::UnexpectedStatus { status, body });
        }

        let mut offsets = vec![0u64];
        let mut bytes_seen = 0u64;
        let mut line_no = 0u64;
        let mut buf: Vec<u8> = Vec::new();

        while let Some(chunk) = response.chunk().await? {
            buf.extend_from_slice(&chunk);
            let mut consumed = 0usize;
            while let Some(rel) = buf[consumed..].iter().position(|&b| b == b'\n') {
                consumed += rel + 1;
                bytes_seen += (rel + 1) as u64;
                line_no += 1;
                if line_no % every == 0 {
                    offsets.push(bytes_seen);
                }
            }
            buf.drain(..consumed);
        }
        if !buf.is_empty() {
            line_no += 1;
        }

        Ok(LineIndex {
            etag: stat.etag,
            size: stat.size,
            checkpoint_every: every,
            total_lines: line_no,
            offsets,
        })
    }

    /// PUT with a WebDAV `If-Match` precondition.
    pub async fn write_file_if_match(
        &self,
        path: &str,
        content: impl Into<bytes::Bytes>,
        etag: &str,
    ) -> Result<()> {
        let url = self.full_url(path)?;
        let response = self
            .client
            .put(url)
            .headers(self.headers())
            .header(reqwest::header::IF_MATCH, etag)
            .body(content.into())
            .send()
            .await?;

        match response.status().as_u16() {
            200 | 201 | 204 => Ok(()),
            412 => Err(WebDavError::PreconditionFailed(etag.to_string())),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(WebDavError::UnexpectedStatus { status, body })
            }
        }
    }

    /// Write through a sibling temp path, then atomically MOVE it over the
    /// target. Parent directories are created through the normal fallback.
    pub async fn write_file_atomic(
        &self,
        path: &str,
        content: impl Into<bytes::Bytes>,
    ) -> Result<()> {
        let content = content.into();
        let tmp = temp_sibling(path);
        self.write_file_with_fallback(&tmp, content).await?;
        match self.move_file_overwrite(&tmp, path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = self.delete(&tmp).await;
                Err(e)
            }
        }
    }

    /// MOVE that is allowed to replace an existing destination.
    pub async fn move_file_overwrite(&self, from: &str, to: &str) -> Result<()> {
        let src_url = self.full_url(from)?;
        let dst_url = self.full_url(to)?;
        let mut headers = self.headers();
        headers.insert(
            "Destination",
            HeaderValue::from_str(dst_url.as_str())
                .map_err(|e| WebDavError::InvalidUrl(format!("Invalid destination URL: {e}")))?,
        );
        headers.insert("Overwrite", HeaderValue::from_static("T"));

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"MOVE").unwrap(), src_url)
            .headers(headers)
            .send()
            .await?;

        self.handle_status(response, |s| s == 201 || s == 204 || s == 200)
            .await?;
        Ok(())
    }

    // ── Nextcloud chunked upload v2 ───────────────────────────────────────────

    /// Start a chunked upload directory. `total` is optional; when supplied it
    /// is sent as `OC-Total-Length` so quota checks run early.
    pub async fn begin_chunked_upload(
        &self,
        dest_path: &str,
        total: Option<u64>,
    ) -> Result<ChunkedUpload> {
        let (prefix, user) = self.upload_root()?;
        let dest_url = self.full_url(dest_path)?.to_string();
        let transfer_id = unique_id();
        let upload_url = format!("{prefix}/uploads/{user}/{transfer_id}");

        let mut headers = self.headers();
        headers.insert(
            "Destination",
            HeaderValue::from_str(&dest_url)
                .map_err(|e| WebDavError::InvalidUrl(format!("Invalid destination URL: {e}")))?,
        );
        if let Some(total) = total {
            headers.insert(
                "OC-Total-Length",
                HeaderValue::from_str(&total.to_string())
                    .map_err(|e| WebDavError::InvalidUrl(format!("Invalid length: {e}")))?,
            );
        }

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &upload_url)
            .headers(headers)
            .send()
            .await?;

        let status = response.status().as_u16();
        if status != 201 && status != 200 && status != 204 {
            let body = response.text().await.unwrap_or_default();
            return Err(WebDavError::UnexpectedStatus { status, body });
        }

        Ok(ChunkedUpload {
            upload_url,
            dest_url,
            total,
            next_chunk: 1,
        })
    }

    /// Upload one chunk (Nextcloud v2 requires 5 MiB–5 GiB except the last).
    pub async fn put_upload_chunk(&self, upload: &mut ChunkedUpload, chunk: &[u8]) -> Result<()> {
        let url = format!("{}/{:05}", upload.upload_url, upload.next_chunk);
        let mut headers = self.headers();
        headers.insert(
            "Destination",
            HeaderValue::from_str(&upload.dest_url)
                .map_err(|e| WebDavError::InvalidUrl(format!("Invalid destination URL: {e}")))?,
        );
        if let Some(total) = upload.total {
            headers.insert(
                "OC-Total-Length",
                HeaderValue::from_str(&total.to_string())
                    .map_err(|e| WebDavError::InvalidUrl(format!("Invalid length: {e}")))?,
            );
        }

        let response = self
            .client
            .put(&url)
            .headers(headers)
            .body(chunk.to_vec())
            .send()
            .await?;

        let status = response.status().as_u16();
        if status != 201 && status != 200 && status != 204 {
            let body = response.text().await.unwrap_or_default();
            return Err(WebDavError::UnexpectedStatus { status, body });
        }
        upload.next_chunk += 1;
        Ok(())
    }

    /// Assemble and move the uploaded chunks onto the destination.
    pub async fn finish_chunked_upload(&self, upload: ChunkedUpload) -> Result<()> {
        let url = format!("{}/.file", upload.upload_url);
        let mut headers = self.headers();
        headers.insert(
            "Destination",
            HeaderValue::from_str(&upload.dest_url)
                .map_err(|e| WebDavError::InvalidUrl(format!("Invalid destination URL: {e}")))?,
        );
        if let Some(total) = upload.total {
            headers.insert(
                "OC-Total-Length",
                HeaderValue::from_str(&total.to_string())
                    .map_err(|e| WebDavError::InvalidUrl(format!("Invalid length: {e}")))?,
            );
        }

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"MOVE").unwrap(), url)
            .headers(headers)
            .send()
            .await?;

        let status = response.status().as_u16();
        if status != 201 && status != 204 && status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(WebDavError::UnexpectedStatus { status, body });
        }
        Ok(())
    }

    /// Remove a partially uploaded chunk directory.
    pub async fn abort_chunked_upload(&self, upload: &ChunkedUpload) -> Result<()> {
        let response = self
            .client
            .delete(format!("{}/", upload.upload_url))
            .headers(self.headers())
            .send()
            .await?;
        let status = response.status().as_u16();
        if status == 404 || status == 204 || status == 200 {
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(WebDavError::UnexpectedStatus { status, body })
        }
    }

    /// Streaming literal replace. Memory stays bounded by `chunk_size` plus one
    /// read chunk; a second match aborts the upload when `replace_all` is false.
    pub async fn rewrite_literal(
        &self,
        path: &str,
        old: &str,
        new: &str,
        replace_all: bool,
        chunk_size: usize,
        expected_etag: Option<&str>,
    ) -> Result<usize> {
        if old.is_empty() {
            return Err(WebDavError::BadRequest(
                "oldString must not be empty".into(),
            ));
        }
        self.check_etag(path, expected_etag).await?;

        let upload = self.begin_chunked_upload(path, None).await?;
        let mut sink = UploadSink::new(self, upload, chunk_size.max(1));
        let mut response = self
            .client
            .get(self.full_url(path)?)
            .headers(self.headers())
            .send()
            .await?;
        let status = response.status().as_u16();
        if status != 200 && status != 206 {
            let body = response.text().await.unwrap_or_default();
            sink.abort().await;
            return Err(WebDavError::UnexpectedStatus { status, body });
        }

        let needle = old.as_bytes();
        let mut carry: Vec<u8> = Vec::new();
        let mut matches = 0usize;

        while let Some(chunk) = response.chunk().await? {
            carry.extend_from_slice(&chunk);
            loop {
                match find_subslice(&carry, needle) {
                    Some(pos) => {
                        if matches >= 1 && !replace_all {
                            sink.abort().await;
                            return Err(WebDavError::BadRequest(
                                "oldString matched more than once".into(),
                            ));
                        }
                        sink.write(&carry[..pos]).await?;
                        sink.write(new.as_bytes()).await?;
                        carry.drain(..pos + needle.len());
                        matches += 1;
                    }
                    None => break,
                }
            }
            let keep = needle.len().saturating_sub(1);
            if carry.len() > keep {
                let drain_len = carry.len() - keep;
                sink.write(&carry[..drain_len]).await?;
                carry.drain(..drain_len);
            }
        }
        sink.write(&carry).await?;

        if matches == 0 {
            sink.abort().await;
            return Ok(0);
        }
        sink.finish().await?;
        Ok(matches)
    }

    /// Streaming line insert / range replacement, used for files that must not
    /// be buffered whole.
    pub async fn rewrite_lines(
        &self,
        path: &str,
        op: LineOp,
        chunk_size: usize,
        expected_etag: Option<&str>,
    ) -> Result<()> {
        self.check_etag(path, expected_etag).await?;

        let upload = self.begin_chunked_upload(path, None).await?;
        let mut sink = UploadSink::new(self, upload, chunk_size.max(1));
        let mut response = self
            .client
            .get(self.full_url(path)?)
            .headers(self.headers())
            .send()
            .await?;
        let status = response.status().as_u16();
        if status != 200 && status != 206 {
            let body = response.text().await.unwrap_or_default();
            sink.abort().await;
            return Err(WebDavError::UnexpectedStatus { status, body });
        }

        let mut buf: Vec<u8> = Vec::new();
        let mut line_no = 0u64;

        while let Some(chunk) = response.chunk().await? {
            buf.extend_from_slice(&chunk);
            let mut consumed = 0usize;
            while let Some(rel) = buf[consumed..].iter().position(|&b| b == b'\n') {
                let end = consumed + rel;
                let raw = &buf[consumed..end];
                consumed = end + 1;
                line_no += 1;
                apply_line_op(&mut sink, &op, line_no, raw, true).await?;
            }
            buf.drain(..consumed);
        }

        if !buf.is_empty() {
            line_no += 1;
            let _ = apply_line_op(&mut sink, &op, line_no, &buf, false).await?;
        }

        match &op {
            LineOp::InsertAfter { line, .. } if *line == 0 || *line > line_no => {
                sink.abort().await;
                return Err(WebDavError::LineOutOfRange(format!(
                    "line {line} is out of range (file has {line_no} lines)"
                )));
            }
            LineOp::ReplaceRange { start, .. } if *start == 0 || *start > line_no => {
                sink.abort().await;
                return Err(WebDavError::LineOutOfRange(format!(
                    "startLine {start} is out of range (file has {line_no} lines)"
                )));
            }
            _ => {}
        }

        sink.finish().await
    }

    async fn check_etag(&self, path: &str, expected: Option<&str>) -> Result<()> {
        if let Some(expected) = expected {
            let stat = self.stat(path).await?;
            if stat.etag.as_deref() != Some(expected) {
                return Err(WebDavError::PreconditionFailed(format!(
                    "expected {expected}, server has {}",
                    stat.etag.as_deref().unwrap_or("<none>")
                )));
            }
        }
        Ok(())
    }

    fn upload_root(&self) -> Result<(String, String)> {
        let idx = self
            .base_url
            .find("/files/")
            .ok_or(WebDavError::ChunkedUploadUnsupported)?;
        let prefix = self.base_url[..idx].to_string();
        let rest = &self.base_url[idx + "/files/".len()..];
        let user = rest
            .split('/')
            .next()
            .filter(|u| !u.is_empty())
            .ok_or(WebDavError::ChunkedUploadUnsupported)?;
        Ok((prefix, user.to_string()))
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
                    etag: prop.getetag.clone(),
                    content_type: prop.getcontenttype.clone(),
                });
            }
        }

        Ok(entries)
    }
}

/// One in-progress Nextcloud chunked upload (v2).
#[derive(Debug, Clone)]
pub struct ChunkedUpload {
    upload_url: String,
    dest_url: String,
    total: Option<u64>,
    next_chunk: u32,
}

/// Streaming line mutation used by [`WebDavClient::rewrite_lines`].
#[derive(Debug, Clone)]
pub enum LineOp {
    InsertAfter { line: u64, text: String },
    ReplaceRange { start: u64, end: u64, text: String },
}

/// Buffers transformed output into 5 MiB-class chunks and uploads them.
struct UploadSink<'a> {
    client: &'a WebDavClient,
    upload: ChunkedUpload,
    buf: Vec<u8>,
    chunk_size: usize,
}

impl<'a> UploadSink<'a> {
    fn new(client: &'a WebDavClient, upload: ChunkedUpload, chunk_size: usize) -> Self {
        Self {
            client,
            upload,
            buf: Vec::new(),
            chunk_size: chunk_size.max(1),
        }
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.buf.extend_from_slice(bytes);
        while self.buf.len() >= self.chunk_size {
            let rest = self.buf.split_off(self.chunk_size);
            self.client
                .put_upload_chunk(&mut self.upload, &self.buf)
                .await?;
            self.buf = rest;
        }
        Ok(())
    }

    async fn finish(mut self) -> Result<()> {
        if !self.buf.is_empty() {
            let buf = std::mem::take(&mut self.buf);
            self.client.put_upload_chunk(&mut self.upload, &buf).await?;
        } else if self.upload.next_chunk == 1 {
            // An empty result still needs one chunk before the assembling MOVE.
            self.client.put_upload_chunk(&mut self.upload, b"").await?;
        }
        let UploadSink { client, upload, .. } = self;
        client.finish_chunked_upload(upload).await
    }

    async fn abort(self) {
        let UploadSink { client, upload, .. } = self;
        let _ = client.abort_chunked_upload(&upload).await;
    }
}

async fn apply_line_op(
    sink: &mut UploadSink<'_>,
    op: &LineOp,
    line_no: u64,
    raw: &[u8],
    terminated: bool,
) -> Result<()> {
    match op {
        LineOp::InsertAfter { line, text } => {
            sink.write(raw).await?;
            if line_no == *line {
                if !terminated {
                    sink.write(b"\n").await?;
                }
                sink.write(text.as_bytes()).await?;
                if terminated && !text.ends_with('\n') {
                    sink.write(b"\n").await?;
                }
            } else if terminated {
                sink.write(b"\n").await?;
            }
            Ok(())
        }
        LineOp::ReplaceRange { start, end, text } => {
            if line_no < *start || line_no > *end {
                sink.write(raw).await?;
                if terminated {
                    sink.write(b"\n").await?;
                }
            } else if line_no == *start && !text.is_empty() {
                sink.write(text.as_bytes()).await?;
                if terminated && !text.ends_with('\n') {
                    sink.write(b"\n").await?;
                }
            }
            Ok(())
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn truncate_line(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max_chars).collect();
    s.push_str(" … (line truncated)");
    s
}

fn content_range_total(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.parse::<u64>().ok())
}

fn temp_sibling(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(pos) => format!("{}/.rockbot-tmp-{}", &trimmed[..pos], unique_id()),
        None => format!(".rockbot-tmp-{}", unique_id()),
    }
}

static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

fn unique_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        nanos,
        UNIQUE_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn truncate_debug(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
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
    fn test_truncate_debug_ascii() {
        let s = "a".repeat(500);
        assert_eq!(truncate_debug(&s, 500), s);
        let out = truncate_debug(&s, 100);
        assert_eq!(out, "a".repeat(100));
    }

    #[test]
    fn test_truncate_debug_utf8_boundary() {
        let s = "字".repeat(300);
        let out = truncate_debug(&s, 100);
        assert_eq!(out.chars().count(), 100);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn test_truncate_debug_shorter_than_limit() {
        assert_eq!(truncate_debug("short", 500), "short");
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
