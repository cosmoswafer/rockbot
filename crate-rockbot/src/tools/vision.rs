use async_trait::async_trait;
use serde_json::Value;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

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

    async fn describe_image(&self, image_url: &str, prompt: &str) -> Result<String> {
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

        let image_bytes = response.bytes().await?;
        let mime_type = detect_mime_type(image_url, &image_bytes);

        Ok(format!(
            "Image downloaded: {} bytes, type: {}. Prompt: {}",
            image_bytes.len(),
            mime_type,
            prompt
        ))
    }
}

fn detect_mime_type(url: &str, _bytes: &[u8]) -> String {
    let url_lower = url.to_lowercase();
    if url_lower.contains(".png") {
        "image/png"
    } else if url_lower.contains(".jpg") || url_lower.contains(".jpeg") {
        "image/jpeg"
    } else if url_lower.contains(".gif") {
        "image/gif"
    } else if url_lower.contains(".webp") {
        "image/webp"
    } else if url_lower.contains(".svg") {
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
        "Download and describe an image. Provide an image URL and an optional prompt."
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
                    "description": "Optional description of what to look for in the image"
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

        self.describe_image(url, prompt).await
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
        assert_eq!(detect_mime_type("test.png", &[]), "image/png");
        assert_eq!(detect_mime_type("test.jpg", &[]), "image/jpeg");
        assert_eq!(detect_mime_type("test.jpeg", &[]), "image/jpeg");
        assert_eq!(detect_mime_type("test.gif", &[]), "image/gif");
    }

    #[tokio::test]
    async fn test_execute_missing_url() {
        let tool = VisionTool::new();
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }
}
