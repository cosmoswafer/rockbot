use async_trait::async_trait;
use base64::Engine;
use serde_json::Value;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(serde::Serialize, serde::Deserialize)]
struct VisionOutput {
    data_uri: String,
    mime_type: String,
    size_bytes: u64,
    prompt: String,
}

pub struct VisionTool {
    http_client: reqwest::Client,
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
        }
    }

    async fn analyze_image(&self, image_url: &str, prompt: &str) -> Result<String> {
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

        if size > MAX_IMAGE_BYTES {
            return Err(RockBotError::Provider(format!(
                "Image too large: {} bytes (max {})",
                size, MAX_IMAGE_BYTES
            )));
        }

        let mime_type = detect_mime_type(image_url, content_type.as_deref());
        let data_uri = format!(
            "data:{};base64,{}",
            mime_type,
            base64::engine::general_purpose::STANDARD.encode(&image_bytes)
        );

        let output = VisionOutput {
            data_uri,
            mime_type,
            size_bytes: size,
            prompt: prompt.to_string(),
        };

        serde_json::to_string(&output).map_err(|e| {
            RockBotError::Provider(format!("Failed to serialize vision output: {}", e))
        })
    }
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
        "Download and analyze an image from a URL using AI vision. Provide an image URL (public web or WebDAV file) and a prompt describing what to look for. User attachments are already visible to you — only use this tool to fetch images from external URLs."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL of the image to analyze"
                },
                "prompt": {
                    "type": "string",
                    "description": "What to look for or ask about in the image"
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

        let prompt = args
            .get("prompt")
            .and_then(|p| p.as_str())
            .unwrap_or("Describe this image in detail.");

        self.analyze_image(url, prompt).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vision_tool_definition() {
        let tool = VisionTool::new();
        assert_eq!(tool.name(), "vision");
        assert!(tool.description().contains("Download"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
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
    fn test_vision_output_serialization() {
        let output = VisionOutput {
            data_uri: "data:image/png;base64,abc".into(),
            mime_type: "image/png".into(),
            size_bytes: 1000,
            prompt: "describe this".into(),
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: VisionOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.data_uri, "data:image/png;base64,abc");
        assert_eq!(parsed.size_bytes, 1000);
        assert_eq!(parsed.prompt, "describe this");
    }
}
