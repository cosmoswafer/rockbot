use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, CONTENT_TYPE};
use reqwest::StatusCode;
use serde::Deserialize;
use tracing::{debug, info};
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::image_cache::{GeneratedImage, ImageCache};
use crate::image_kind::ImageKind;
use crate::tool::Tool;
use crate::utils::uuid_string;
use crate::validated::NonEmptyString;

/// Share a WebDAV image into the current chat.
///
/// SVG is served through the share's `/download` endpoint when Nextcloud
/// returns `image/svg+xml`; otherwise the tool falls back to Nextcloud's
/// server-side `/preview` rasterizer (issue #122). Raster formats keep the
/// normal `/download` share link.
pub struct ShareImageTool {
    webdav: WebDavClient,
    image_cache: Arc<ImageCache>,
    http: reqwest::Client,
    max_attachment_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct ShareImageParams {
    path: NonEmptyString,
    #[serde(default)]
    room_id: Option<String>,
    #[serde(default)]
    webdav_dir: Option<String>,
    /// Injected by the harness as `tool_call.id`, exactly like `image_gen`.
    #[serde(default)]
    image_cache_key: Option<String>,
}

impl ShareImageTool {
    pub fn new(
        webdav: WebDavClient,
        image_cache: Arc<ImageCache>,
        max_attachment_bytes: u64,
    ) -> Self {
        Self {
            webdav,
            image_cache,
            http: reqwest::Client::new(),
            max_attachment_bytes,
        }
    }

    fn room_path(&self, dir_key: &str, subpath: &str) -> Result<String> {
        WebDavPath::new("")
            .room_path(dir_key, subpath)
            .map_err(|e| RockBotError::ToolCallParse(format!(
                "share_image: path traversal rejected: {e}"
            )))
    }

    async fn share(&self, params: &ShareImageParams) -> Result<String> {
        let dir = params
            .webdav_dir
            .as_deref()
            .or(params.room_id.as_deref())
            .unwrap_or("unknown");
        let source = self.room_path(dir, params.path.as_str())?;
        let kind = ImageKind::from_path(&source).ok_or_else(|| {
            RockBotError::ToolCallParse(format!(
                "share_image: unsupported image extension for '{source}' — \
                 supported: png, jpg, jpeg, gif, webp, svg"
            ))
        })?;

        let stat = self
            .webdav
            .stat(&source)
            .await
            .map_err(|e| RockBotError::Provider(format!("share_image: stat failed for {source}: {e}")))?;
        if stat.is_dir {
            return Err(RockBotError::ToolCallParse(format!(
                "share_image: '{source}' is a directory, not an image"
            )));
        }
        if stat.size > self.max_attachment_bytes {
            return Err(RockBotError::ToolCallParse(format!(
                "share_image: {source} is {} bytes, above the {} byte limit",
                stat.size, self.max_attachment_bytes
            )));
        }

        let download_url = self
            .webdav
            .create_nextcloud_share_link(&source)
            .await
            .ok_or_else(|| {
                RockBotError::Provider(format!(
                    "share_image: could not create a Nextcloud share for '{source}' — \
                     check the Nextcloud sharing API"
                ))
            })?;

        let (image_bytes, mime_type, display_url) = if kind.is_vector() {
            match fetch_svg_download(&self.http, &download_url, self.max_attachment_bytes).await? {
                // Vector first: no raster, no animation loss, no WebDAV re-read.
                SvgDownload::Vector(bytes) => (bytes, kind.mime().to_string(), download_url),
                SvgDownload::Unsupported(reason) => {
                    debug!(
                        "share_image: {download_url} is not displayable as SVG ({reason}) — \
                         falling back to /preview"
                    );
                    let preview = preview_url(&download_url).ok_or_else(|| {
                        RockBotError::Provider(format!(
                            "share_image: unexpected Nextcloud share URL '{download_url}'"
                        ))
                    })?;
                    let (bytes, content_type) =
                        fetch_preview(&self.http, &preview, self.max_attachment_bytes)
                            .await
                            .map_err(|e| {
                                RockBotError::Provider(format!(
                                    "share_image: SVG download unusable ({reason}) and /preview \
                                     fallback failed: {e} — check config/mimetypemapping.json + \
                                     occ maintenance:mimetype:update-db, and enabledPreviewProviders"
                                ))
                            })?;
                    (bytes, content_type, preview)
                }
            }
        } else {
            let bytes = self.webdav.read_file(&source).await.map_err(|e| {
                RockBotError::Provider(format!("share_image: read failed for {source}: {e}"))
            })?;
            if bytes.len() as u64 > self.max_attachment_bytes {
                return Err(RockBotError::ToolCallParse(format!(
                    "share_image: {source} is {} bytes, above the {} byte limit",
                    bytes.len(),
                    self.max_attachment_bytes
                )));
            }
            (bytes, kind.mime().to_string(), download_url)
        };

        let image_key = params
            .image_cache_key
            .clone()
            .unwrap_or_else(uuid_string);

        self.image_cache.store(
            &image_key,
            GeneratedImage {
                webdav_path: NonEmptyString::try_new(source.clone())
                    .expect("source path is non-empty"),
                image_bytes,
                mime_type: NonEmptyString::try_new(mime_type.clone())
                    .expect("preview content-type is non-empty"),
                share_url: Some(display_url.clone()),
            },
        );

        info!(
            "share_image: {} -> {} (kind={:?}, image_key={})",
            source, display_url, kind, image_key
        );

        Ok(serde_json::json!({
            "ok": true,
            "image_key": image_key,
            "source_path": source,
            "display_url": display_url,
            "mime_type": mime_type,
        })
        .to_string())
    }
}

/// `create_nextcloud_share_link()` always returns `{share}/download`.
/// Nextcloud exposes the server-side rasterized preview at `{share}/preview`.
fn preview_url(download_url: &str) -> Option<String> {
    download_url
        .strip_suffix("/download")
        .map(|base| format!("{base}/preview"))
}

/// Why `{share}/download` cannot be used as a vector display URL.
#[derive(Debug)]
enum DownloadRejection {
    /// Non-2xx final status (reqwest already followed redirects).
    Status(StatusCode),
    /// 2xx but the final `Content-Type` is not `image/svg+xml`.
    Mime(String),
    /// DNS/TLS/timeout/transport failure.
    Transport(String),
}

impl fmt::Display for DownloadRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(s) => write!(f, "HTTP {s}"),
            Self::Mime(m) => write!(f, "content-type {m:?}"),
            Self::Transport(e) => write!(f, "transport error: {e}"),
        }
    }
}

/// One-shot result of probing `{share}/download`.
enum SvgDownload {
    /// `2xx` + `image/svg+xml`; these are the bytes to display.
    Vector(Vec<u8>),
    /// Not displayable as vector; the caller falls back to `/preview`.
    Unsupported(DownloadRejection),
}

/// Lowercase the bare MIME type, dropping parameters such as `; charset=UTF-8`.
fn normalized_mime(headers: &HeaderMap) -> String {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// GET `{share}/download` once. A `2xx` + `image/svg+xml` response returns the
/// body as the display bytes; any other outcome is reported so the caller can
/// fall back to `/preview`. Transport failures are reported as `Unsupported`;
/// size-cap and mid-stream read failures are hard tool errors.
async fn fetch_svg_download(
    http: &reqwest::Client,
    url: &str,
    max_bytes: u64,
) -> Result<SvgDownload> {
    let response = match http.get(url).timeout(Duration::from_secs(30)).send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(SvgDownload::Unsupported(DownloadRejection::Transport(
                e.to_string(),
            )));
        }
    };

    let status = response.status();
    let content_type = normalized_mime(response.headers());
    if !status.is_success() {
        return Ok(SvgDownload::Unsupported(DownloadRejection::Status(status)));
    }
    if content_type != "image/svg+xml" {
        return Ok(SvgDownload::Unsupported(DownloadRejection::Mime(
            content_type,
        )));
    }

    let bytes = read_body_capped(response, max_bytes, "SVG").await?;
    if bytes.is_empty() {
        return Err(RockBotError::Provider(
            "share_image: Nextcloud share download returned an empty SVG body".into(),
        ));
    }
    debug!(
        "share_image: /download served SVG ({} bytes, content-type={content_type})",
        bytes.len()
    );
    Ok(SvgDownload::Vector(bytes))
}

/// Read a response body without ever buffering more than `max_bytes`.
async fn read_body_capped(
    mut response: reqwest::Response,
    max_bytes: u64,
    what: &str,
) -> Result<Vec<u8>> {
    if let Some(len) = response.content_length()
        && len > max_bytes
    {
        return Err(RockBotError::ToolCallParse(format!(
            "share_image: Nextcloud {what} is {len} bytes, above the {max_bytes} byte limit"
        )));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        RockBotError::Provider(format!("share_image: Nextcloud {what} read failed: {e}"))
    })? {
        if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(RockBotError::ToolCallParse(format!(
                "share_image: Nextcloud {what} is above the {max_bytes} byte limit"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Fetch and validate a Nextcloud public preview.
///
/// The endpoint must return `2xx` with an `image/*` content type; anything
/// else (preview disabled, HTML error page, oversized body) is a tool error
/// rather than a broken image key.
async fn fetch_preview(
    http: &reqwest::Client,
    url: &str,
    max_bytes: u64,
) -> Result<(Vec<u8>, String)> {
    let response = http
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| {
            RockBotError::Provider(format!("share_image: Nextcloud preview request failed: {e}"))
        })?;

    let status = response.status();
    let mime = normalized_mime(response.headers());

    if !status.is_success() || !mime.starts_with("image/") {
        return Err(RockBotError::Provider(format!(
            "share_image: Nextcloud preview unavailable ({status}, content-type={mime:?}) — \
             check the Nextcloud preview settings (enabledPreviewProviders)"
        )));
    }

    let bytes = read_body_capped(response, max_bytes, "preview").await?;
    debug!(
        "share_image: preview ok ({} bytes, content-type={})",
        bytes.len(),
        mime
    );
    Ok((bytes, mime))
}

#[async_trait]
impl Tool for ShareImageTool {
    fn name(&self) -> &str {
        "share_image"
    }

    fn description(&self) -> &str {
        "Share an image stored in the room's WebDAV workspace into the chat. \
         SVG is served through the share's /download endpoint when Nextcloud \
         returns image/svg+xml (vector, animation preserved); otherwise it falls \
         back to the server-side /preview rasterizer. PNG/JPEG/GIF/WebP use the \
         normal /download share link. \
         Returns {\"ok\":true,\"image_key\":\"...\"}. \
         Include ![description]({image_key}) in your reply to display it."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Image path relative to the room's WebDAV directory, e.g. 'diagram.svg' or 'images/chart.png'"
                },
                "webdav_dir": {
                    "type": "string",
                    "description": "Room WebDAV directory key (injected automatically)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let params: ShareImageParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse share_image arguments: {e}"))
        })?;
        debug!("share_image execute: path={}", params.path.as_str());
        self.share(&params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_url_replaces_download_suffix() {
        assert_eq!(
            preview_url("https://nc.example/s/abc/download"),
            Some("https://nc.example/s/abc/preview".to_string())
        );
    }

    #[test]
    fn preview_url_rejects_unexpected_shapes() {
        assert_eq!(preview_url("https://nc.example/s/abc"), None);
        assert_eq!(preview_url("https://nc.example/s/abc/download?x=1"), None);
    }

    #[test]
    fn normalized_mime_drops_parameters_and_case() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "IMAGE/SVG+XML; charset=UTF-8".parse().unwrap());
        assert_eq!(normalized_mime(&headers), "image/svg+xml");

        assert_eq!(normalized_mime(&HeaderMap::new()), "");
    }

    #[test]
    fn tool_definition_exposes_path_only() {
        let client = WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ShareImageTool::new(client, Arc::new(ImageCache::new()), 1024);
        assert_eq!(tool.name(), "share_image");
        assert!(tool.description().contains("/download"));
        assert!(tool.description().contains("/preview"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("path"))
        );
        assert!(params["properties"].get("path").is_some());
        assert!(params["properties"].get("image_cache_key").is_none());
    }

    #[tokio::test]
    async fn rejects_unsupported_extension_before_network() {
        let client = WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ShareImageTool::new(client, Arc::new(ImageCache::new()), 1024);
        let err = tool
            .execute(r#"{"path":"notes.txt","webdav_dir":"r-test"}"#)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unsupported image extension"));
    }

    #[tokio::test]
    async fn rejects_missing_path() {
        let client = WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ShareImageTool::new(client, Arc::new(ImageCache::new()), 1024);
        assert!(tool.execute("{}").await.is_err());
    }
}
