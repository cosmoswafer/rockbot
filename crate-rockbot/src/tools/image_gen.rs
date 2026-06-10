use async_trait::async_trait;
use base64::Engine;
use serde_json::Value;
use tracing::{debug, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::provider::fal::{FalAiProvider, ImageGenParams, ImageSizeValue};
use crate::tool::Tool;

pub struct ImageGenTool {
    fal: FalAiProvider,
    fal_img2img: Option<FalAiProvider>,
    default_quality: String,
    webdav: WebDavClient,
    http_client: reqwest::Client,
}

impl ImageGenTool {
    pub fn new(fal: FalAiProvider, default_quality: String, webdav: WebDavClient) -> Self {
        Self {
            fal,
            fal_img2img: None,
            default_quality,
            webdav,
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_img2img(
        fal_text2img: FalAiProvider,
        fal_img2img: FalAiProvider,
        default_quality: String,
        webdav: WebDavClient,
    ) -> Self {
        Self {
            fal: fal_text2img,
            fal_img2img: Some(fal_img2img),
            default_quality,
            webdav,
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_client(fal: FalAiProvider, webdav: WebDavClient, client: reqwest::Client) -> Self {
        Self {
            fal,
            fal_img2img: None,
            default_quality: "medium".into(),
            webdav,
            http_client: client,
        }
    }

    async fn upload_data_uri(&self, data_uri: &str) -> Result<String> {
        // Parse data URI: data:<mime>;base64,<data>
        let after_data = data_uri
            .strip_prefix("data:")
            .ok_or_else(|| RockBotError::ToolCallParse("Invalid data URI".into()))?;
        let (mime_part, b64) = after_data
            .split_once(";base64,")
            .ok_or_else(|| RockBotError::ToolCallParse("Data URI missing ;base64,".into()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| RockBotError::ToolCallParse(format!("Base64 decode failed: {e}")))?;
        self.fal.upload_file(&bytes, mime_part).await
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

    async fn upload_to_webdav(&self, room_id: &str, ext: &str, image_bytes: Vec<u8>) -> Result<String> {
        let filename = WebDavPath::new("").image_path(room_id, &format!("{}.{}", uuid_string(), ext));
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

fn ext_from_output_format(output_format: Option<&str>) -> &str {
    match output_format {
        Some("jpeg") | Some("jpg") => "jpg",
        Some("png") => "png",
        Some("webp") => "webp",
        _ => "png",
    }
}

#[async_trait]
impl Tool for ImageGenTool {
    fn name(&self) -> &str {
        "image_gen"
    }

    fn description(&self) -> &str {
        "Generate or edit an image. For text-to-image, provide a prompt. \
         To edit or transform an image the user sent, just describe what to do \
         in the prompt — the user's image attachments will be automatically \
         provided as image_urls input. Optional parameters: quality, image_size, \
         output_format, num_images. \
         Returns both the WebDAV path and the original fal.ai CDN URL — prefer \
         the fal.ai URL when sharing the image with the user."
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
                "quality": {
                    "type": "string",
                    "enum": ["low", "medium", "high", "auto"],
                    "description": "Image quality / reasoning budget. Default: medium. For gpt-image-2, medium is recommended for cost balance."
                },
                "image_size": {
                    "type": "string",
                    "description": "Aspect ratio preset or custom {\\\"width\\\": N, \\\"height\\\": N} JSON. Presets: square_hd (1:1 2880x2880), square (512x512), landscape_16_9 (3840x2160 4K), portrait_16_9 (2160x3840), landscape_4_3 (3312x2480), portrait_4_3 (2480x3312), landscape_3_2 (3504x2336), portrait_2_3 (2336x3504), auto. Default: landscape_4_3. Max edge 3840px, multiples of 16."
                },
                "output_format": {
                    "type": "string",
                    "enum": ["jpeg", "png", "webp"],
                    "description": "Output image format. Default: png."
                },
                "num_images": {
                    "type": "integer",
                    "description": "Number of images to generate per request. Default: 1."
                },
                "image_urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Image URLs for editing/transformations. When the user sends images, they are automatically injected. Do NOT try to reference data URIs from vision context — they will be provided automatically."
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

        let mut params = ImageGenParams::new(prompt);
        params.quality = Some(
            args.get("quality")
                .and_then(|v| v.as_str())
                .unwrap_or(&self.default_quality)
                .to_string(),
        );
        params.output_format = args.get("output_format").and_then(|v| v.as_str()).map(String::from);
        params.num_images = args.get("num_images").and_then(|v| v.as_u64()).map(|n| n as u32);

        if let Some(size_val) = args.get("image_size") {
            params.image_size = size_val.as_str().map(|s| ImageSizeValue::Preset(s.to_string())).or_else(|| {
                size_val.as_object().map(|obj| {
                    let w = obj.get("width").and_then(|w| w.as_u64()).unwrap_or(1024) as u32;
                    let h = obj.get("height").and_then(|h| h.as_u64()).unwrap_or(768) as u32;
                    ImageSizeValue::Custom { width: w, height: h }
                })
            });
        }

        if let Some(image_urls) = args.get("image_urls").and_then(|v| v.as_array()) {
            const MAX_DATA_URI_BYTES: usize = 5_000_000;
            let mut urls: Vec<String> = Vec::with_capacity(image_urls.len());
            for v in image_urls {
                let raw = v.as_str().map(String::from);
                match raw.as_deref() {
                    Some(uri) if uri.len() > MAX_DATA_URI_BYTES && uri.starts_with("data:") => {
                        if let Ok(uploaded_url) = self.upload_data_uri(uri).await {
                            debug!("Uploaded oversized image to fal storage: {}", uploaded_url);
                            urls.push(uploaded_url);
                        } else {
                            warn!("Failed to upload oversized data URI, skipping it");
                        }
                    }
                    Some(s) => urls.push(s.to_string()),
                    None => {}
                }
            }
            if !urls.is_empty() {
                params.image_urls = Some(urls);
            }
        }

        let ext = ext_from_output_format(params.output_format.as_deref());

        let provider = if params.image_urls.is_some() {
            self.fal_img2img.as_ref().unwrap_or(&self.fal)
        } else {
            &self.fal
        };

        debug!(
            "Generating image with fal.ai model={}: {}",
            provider.model_id(),
            prompt
        );

        let image_url = provider.generate_image(&params).await?;
        debug!("Image generated, URL: {}", image_url);
        let image_bytes = self.download_image(&image_url).await?;

        let webdav_path = self.upload_to_webdav(webdav_dir, ext, image_bytes).await?;

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
        let tool = ImageGenTool::new(fal, "medium".into(), webdav);

        assert_eq!(tool.name(), "image_gen");
        assert!(tool.description().contains("Generate or edit an image"));
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("prompt"))
        );
        // verify new optional parameters exist
        assert!(params["properties"].get("quality").is_some());
        assert!(params["properties"].get("image_size").is_some());
        assert!(params["properties"].get("output_format").is_some());
        assert!(params["properties"].get("num_images").is_some());
        assert!(params["properties"].get("image_urls").is_some());
    }

    #[tokio::test]
    async fn test_execute_missing_prompt() {
        let config = make_fal_config();
        let fal = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ImageGenTool::new(fal, "medium".into(), webdav);
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let config = make_fal_config();
        let fal = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ImageGenTool::new(fal, "medium".into(), webdav);
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

    #[test]
    fn test_ext_from_output_format_default() {
        assert_eq!(ext_from_output_format(None), "png");
        assert_eq!(ext_from_output_format(Some("png")), "png");
        assert_eq!(ext_from_output_format(Some("jpeg")), "jpg");
        assert_eq!(ext_from_output_format(Some("webp")), "webp");
        assert_eq!(ext_from_output_format(Some("unknown")), "png");
    }

    #[test]
    fn test_image_gen_params_from_args() {
        let args: Value = serde_json::from_str(r#"{
            "prompt": "a cat",
            "quality": "medium",
            "image_size": "landscape_16_9",
            "output_format": "png",
            "num_images": 2
        }"#).unwrap();

        let mut params = ImageGenParams::new(args["prompt"].as_str().unwrap());
        params.quality = args.get("quality").and_then(|v| v.as_str()).map(String::from);
        params.output_format = args.get("output_format").and_then(|v| v.as_str()).map(String::from);
        params.num_images = args.get("num_images").and_then(|v| v.as_u64()).map(|n| n as u32);
        if let Some(size_val) = args.get("image_size") {
            params.image_size = size_val.as_str().map(|s| ImageSizeValue::Preset(s.to_string()));
        }

        assert_eq!(params.quality.as_deref(), Some("medium"));
        assert_eq!(params.num_images, Some(2));

        let resolved = params.resolve_image_size().unwrap();
        assert_eq!(resolved["width"], 3840);
        assert_eq!(resolved["height"], 2160);
    }

    #[test]
    fn test_image_gen_params_custom_size() {
        let mut params = ImageGenParams::new("test");
        params.image_size = Some(ImageSizeValue::Custom { width: 1920, height: 1080 });
        let resolved = params.resolve_image_size().unwrap();
        assert_eq!(resolved["width"], 1920);
        assert_eq!(resolved["height"], 1080);
    }

    #[test]
    fn test_image_gen_params_no_optional() {
        let args: Value = serde_json::from_str(r#"{"prompt": "a cat"}"#).unwrap();
        let params = ImageGenParams::new(args["prompt"].as_str().unwrap());

        assert!(params.quality.is_none());
        assert!(params.output_format.is_none());
        assert!(params.num_images.is_none());
        assert!(params.image_size.is_none());
    }

    #[test]
    fn test_image_gen_params_with_image_urls() {
        let args: Value = serde_json::from_str(r#"{
            "prompt": "edit this image",
            "image_urls": ["https://example.com/img1.png", "data:image/png;base64,abc"]
        }"#).unwrap();

        let mut params = ImageGenParams::new(args["prompt"].as_str().unwrap());
        if let Some(image_urls) = args.get("image_urls").and_then(|v| v.as_array()) {
            let urls: Vec<String> = image_urls
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !urls.is_empty() {
                params.image_urls = Some(urls);
            }
        }

        let urls = params.image_urls.unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/img1.png");
        assert_eq!(urls[1], "data:image/png;base64,abc");
    }

    #[test]
    fn test_image_gen_params_empty_image_urls() {
        let args: Value = serde_json::from_str(r#"{"prompt": "test", "image_urls": []}"#).unwrap();
        let mut params = ImageGenParams::new(args["prompt"].as_str().unwrap());
        if let Some(image_urls) = args.get("image_urls").and_then(|v| v.as_array()) {
            let urls: Vec<String> = image_urls
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !urls.is_empty() {
                params.image_urls = Some(urls);
            }
        }
        assert!(params.image_urls.is_none());
    }

    #[test]
    fn test_image_gen_params_no_image_urls() {
        let args: Value = serde_json::from_str(r#"{"prompt": "test"}"#).unwrap();
        let mut params = ImageGenParams::new(args["prompt"].as_str().unwrap());
        if let Some(image_urls) = args.get("image_urls").and_then(|v| v.as_array()) {
            let urls: Vec<String> = image_urls
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !urls.is_empty() {
                params.image_urls = Some(urls);
            }
        }
        assert!(params.image_urls.is_none());
    }
}
