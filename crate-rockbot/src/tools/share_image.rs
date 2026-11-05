use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
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
/// SVG is displayed through Nextcloud's server-side `/preview` rasterizer
/// (issue #120). Raster formats keep the normal `/download` share link.
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
            let preview = preview_url(&download_url).ok_or_else(|| {
                RockBotError::Provider(format!(
                    "share_image: unexpected Nextcloud share URL '{download_url}'"
                ))
            })?;
            let (bytes, content_type) =
                fetch_preview(&self.http, &preview, self.max_attachment_bytes).await?;
            (bytes, content_type, preview)
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
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    if !status.is_success() || !mime.starts_with("image/") {
        return Err(RockBotError::Provider(format!(
            "share_image: Nextcloud preview unavailable ({status}, content-type={content_type:?}) — \
             check the Nextcloud preview settings (enabledPreviewProviders)"
        )));
    }

    if let Some(len) = response.content_length() {
        if len > max_bytes {
            return Err(RockBotError::ToolCallParse(format!(
                "share_image: preview is {len} bytes, above the {max_bytes} byte limit"
            )));
        }
    }

    let bytes = response.bytes().await.map_err(|e| {
        RockBotError::Provider(format!("share_image: Nextcloud preview read failed: {e}"))
    })?;
    if bytes.len() as u64 > max_bytes {
        return Err(RockBotError::ToolCallParse(format!(
            "share_image: preview is {} bytes, above the {max_bytes} byte limit",
            bytes.len()
        )));
    }

    debug!(
        "share_image: preview ok ({} bytes, content-type={})",
        bytes.len(),
        mime
    );
    Ok((bytes.to_vec(), mime))
}

#[async_trait]
impl Tool for ShareImageTool {
    fn name(&self) -> &str {
        "share_image"
    }

    fn description(&self) -> &str {
        "Share an image stored in the room's WebDAV workspace into the chat. \
         SVG is displayed through Nextcloud's server-side /preview rasterizer; \
         PNG/JPEG/GIF/WebP use the normal /download share link. \
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
    fn tool_definition_exposes_path_only() {
        let client = WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ShareImageTool::new(client, Arc::new(ImageCache::new()), 1024);
        assert_eq!(tool.name(), "share_image");
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
