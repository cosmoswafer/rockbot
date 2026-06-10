use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::provider::FalAiProvider;
use crate::tool::Tool;

pub struct ImageGenTool {
    fal: FalAiProvider,
    webdav: WebDavClient,
    http_client: reqwest::Client,
}

impl ImageGenTool {
    pub fn new(fal: FalAiProvider, webdav: WebDavClient) -> Self {
        Self {
            fal,
            webdav,
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_client(fal: FalAiProvider, webdav: WebDavClient, client: reqwest::Client) -> Self {
        Self {
            fal,
            webdav,
            http_client: client,
        }
    }

    async fn download_image(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .http_client
            .get(url)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| {
                RockBotError::Provider(format!("Failed to download generated image: {e}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(RockBotError::Provider(format!(
                "Failed to download generated image: HTTP {}",
                status
            )));
        }

        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| RockBotError::Provider(format!("Failed to read image bytes: {e}")))
    }

    async fn upload_to_webdav(&self, room_id: &str, image_bytes: Vec<u8>) -> Result<String> {
        let filename = WebDavPath::new("").image_path(room_id, &format!("{}.png", uuid_string()));
        debug!("Uploading generated image to WebDAV: {}", filename);
        self.webdav
            .write_file_with_fallback(&filename, image_bytes)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV upload failed: {e}")))?;
        Ok(filename)
    }
}

fn uuid_string() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{:08x}-{:04x}",
        now.as_secs() as u32,
        now.subsec_millis() as u16
    )
}

#[async_trait]
impl Tool for ImageGenTool {
    fn name(&self) -> &str {
        "image_gen"
    }

    fn description(&self) -> &str {
        "Generate an image using fal.ai. Specify a prompt and an optional model_id \
         (defaults to fal-ai/flux/schnell for fast generation). \
         Images are stored on WebDAV and the path is returned."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Text description of the image to generate"
                },
                "room_id": {
                    "type": "string",
                    "description": "Room ID for image storage (injected automatically if omitted)"
                },
                "model_id": {
                    "type": "string",
                    "description": "fal.ai model ID (default: fal-ai/flux/schnell)"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse image_gen arguments: {e}"))
        })?;

        let prompt = args.get("prompt").and_then(|p| p.as_str()).ok_or_else(|| {
            RockBotError::ToolCallParse("image_gen requires 'prompt' field".into())
        })?;

        let room_id = args
            .get("room_id")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown");

        let webdav_dir = args
            .get("webdav_dir")
            .and_then(|d| d.as_str())
            .unwrap_or(room_id);

        debug!(
            "Generating image with fal.ai model={}: {}",
            self.fal.model_id(),
            prompt
        );

        let image_url = self.fal.generate_image(prompt).await?;
        debug!("Image generated, URL: {}", image_url);

        let image_bytes = self.download_image(&image_url).await?;

        let webdav_path = self.upload_to_webdav(webdav_dir, image_bytes).await?;

        Ok(format!(
            "Image generated and stored at {}. Original fal.ai URL: {}",
            webdav_path, image_url
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;
    use std::collections::HashMap;

    fn make_fal_config() -> ProviderConfig {
        ProviderConfig {
            name: "fal".into(),
            api_key: "test-key".into(),
            base_url: "https://queue.fal.run".into(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: HashMap::new(),
        }
    }

    #[test]
    fn test_image_gen_tool_definition() {
        let config = make_fal_config();
        let fal = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ImageGenTool::new(fal, webdav);

        assert_eq!(tool.name(), "image_gen");
        assert!(tool.description().contains("Generate an image"));
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("prompt"))
        );
    }

    #[tokio::test]
    async fn test_execute_missing_prompt() {
        let config = make_fal_config();
        let fal = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ImageGenTool::new(fal, webdav);
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let config = make_fal_config();
        let fal = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ImageGenTool::new(fal, webdav);
        let result = tool.execute("not json").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_uuid_string_format() {
        let id = uuid_string();
        assert!(id.contains('-'));
        assert_eq!(id.len(), 13);
    }

    #[test]
    fn test_webdav_dir_extraction() {
        let args = serde_json::json!({
            "prompt": "a cat",
            "room_id": "uuid-123",
            "webdav_dir": "d-saru"
        });
        assert_eq!(args["webdav_dir"], "d-saru");
        assert_eq!(args["room_id"], "uuid-123");
    }

    #[test]
    fn test_webdav_dir_fallback_to_room_id() {
        let args = serde_json::json!({
            "prompt": "a cat",
            "room_id": "uuid-123"
        });
        assert!(args.get("webdav_dir").is_none());
        let webdav_dir = args
            .get("webdav_dir")
            .and_then(|d| d.as_str())
            .unwrap_or(args["room_id"].as_str().unwrap());
        assert_eq!(webdav_dir, "uuid-123");
    }
}
