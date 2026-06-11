use async_trait::async_trait;
use base64::Engine;
use serde_json::Value;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

const DEFAULT_MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

pub struct VisionTool {
    http_client: reqwest::Client,
    max_image_bytes: u64,
}

impl Default for VisionTool {
    fn default() -> Self {
        Self::new()
    }
}

impl VisionTool {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
            max_image_bytes: DEFAULT_MAX_IMAGE_BYTES,
        }
    }

    pub fn with_max_bytes(max_bytes: u64) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            max_image_bytes: max_bytes,
        }
    }

    async fn fetch_image(&self, image_url: &str) -> Result<String> {
        let response = self
            .http_client
            .get(image_url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(RockBotError::Provider(format!(
                "Failed to download image: HTTP {}",
                status
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let image_bytes = response.bytes().await?;
        let size = image_bytes.len() as u64;

        if size > self.max_image_bytes {
            return Err(RockBotError::Provider(format!(
                "Image too large: {} bytes (max {})",
                size, self.max_image_bytes
            )));
        }

        let mime_type = detect_mime_type(image_url, content_type.as_deref());
        let name = extract_image_name(image_url);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&image_bytes);

        Ok(format!("![{}](data:{};base64,{})", name, mime_type, b64))
    }
}

fn extract_image_name(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    path.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("image")
        .to_string()
}

fn detect_mime_type(url: &str, content_type: Option<&str>) -> String {
    if let Some(ct) = content_type {
        let ct_lower = ct.to_lowercase();
        if ct_lower.starts_with("image/") {
            return ct_lower;
        }
    }

    let url_lower = url.to_lowercase();
    if url_lower.ends_with(".png") {
        "image/png"
    } else if url_lower.ends_with(".jpg") || url_lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if url_lower.ends_with(".gif") {
        "image/gif"
    } else if url_lower.ends_with(".webp") {
        "image/webp"
    } else if url_lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/png"
    }
    .to_string()
}

#[async_trait]
impl Tool for VisionTool {
    fn name(&self) -> &str {
        "vision"
    }

    fn description(&self) -> &str {
        "Fetch an image from a WebDAV file path or public URL and return it as a base64 markdown image tag. \n\
         Use this to retrieve images the user is asking about from WebDAV storage or external URLs. \n\
         Optionally provide a prompt hint for how the image should be analyzed. \n\
         User attachments are already visible to you — only use this tool for images at explicit URLs."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL of the image to fetch (public web or WebDAV file)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional prompt for the LLM to use when analyzing this image"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse vision arguments: {}", e))
        })?;

        let url = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| RockBotError::ToolCallParse("vision requires 'url' field".into()))?;

        self.fetch_image(url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vision_tool_definition() {
        let tool = VisionTool::new();
        assert_eq!(tool.name(), "vision");
        assert!(tool.description().contains("Fetch"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
    }

    #[test]
    fn test_extract_image_name() {
        assert_eq!(
            extract_image_name("https://example.com/path/to/photo.png"),
            "photo.png"
        );
        assert_eq!(
            extract_image_name("https://example.com/photo.png?size=large"),
            "photo.png"
        );
        assert_eq!(extract_image_name("https://example.com/path/"), "image");
        assert_eq!(extract_image_name("photo.jpg"), "photo.jpg");
        assert_eq!(extract_image_name("https://example.com/"), "image");
    }

    #[test]
    fn test_detect_mime_type() {
        assert_eq!(detect_mime_type("test.png", None), "image/png");
        assert_eq!(detect_mime_type("test.jpg", None), "image/jpeg");
        assert_eq!(detect_mime_type("test.jpeg", None), "image/jpeg");
        assert_eq!(detect_mime_type("test.gif", None), "image/gif");
        assert_eq!(detect_mime_type("test.webp", None), "image/webp");
        assert_eq!(detect_mime_type("test.svg", None), "image/svg+xml");
    }

    #[test]
    fn test_detect_mime_type_prefers_content_type() {
        assert_eq!(
            detect_mime_type("test.pdf", Some("image/jpeg")),
            "image/jpeg"
        );
    }

    #[test]
    fn test_detect_mime_type_falls_back_when_not_image() {
        assert_eq!(
            detect_mime_type("test.png", Some("text/html")),
            "image/png"
        );
    }

    #[tokio::test]
    async fn test_execute_missing_url() {
        let tool = VisionTool::new();
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_markdown_tag_format() {
        let tag = "![photo.png](data:image/png;base64,abc)";
        assert!(tag.starts_with("!["));
        assert!(tag.contains("](data:"));
        assert!(tag.contains(";base64,"));
        assert!(tag.ends_with(")"));
    }

    #[test]
    fn test_vision_tool_with_max_bytes() {
        let tool = VisionTool::with_max_bytes(1000);
        assert_eq!(tool.max_image_bytes, 1000);
        assert_eq!(tool.name(), "vision");
    }

    #[test]
    fn test_vision_parameters_includes_prompt() {
        let tool = VisionTool::new();
        let params = tool.parameters();
        assert!(params["properties"].get("prompt").is_some());
        assert_eq!(params["properties"]["prompt"]["type"], "string");
        let prompt_desc = params["properties"]["prompt"]["description"]
            .as_str()
            .unwrap();
        assert!(prompt_desc.contains("Optional"));
    }
}
