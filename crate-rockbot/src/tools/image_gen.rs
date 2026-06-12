use async_trait::async_trait;
use base64::Engine;
use serde_json::Value;
use tracing::{debug, info, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::image_cache::{GeneratedImage, ImageCache};
use crate::provider::ImageProvider;
use crate::tool::Tool;
use crate::types::{ImageGenParams, ImageSizeValue};

use std::sync::Arc;

pub struct ImageGenTool {
    provider: Box<dyn ImageProvider>,
    edit_provider: Option<Box<dyn ImageProvider>>,
    default_quality: String,
    default_output_format: String,
    default_num_images: u32,
    default_image_size: String,
    default_image_size_tier: String,
    webdav: WebDavClient,
    image_cache: Arc<ImageCache>,
}

impl ImageGenTool {
    pub fn new(
        provider: Box<dyn ImageProvider>,
        default_quality: String,
        default_output_format: String,
        default_num_images: u32,
        default_image_size: String,
        default_image_size_tier: String,
        webdav: WebDavClient,
        image_cache: Arc<ImageCache>,
    ) -> Self {
        Self {
            provider,
            edit_provider: None,
            default_quality,
            default_output_format,
            default_num_images,
            default_image_size,
            default_image_size_tier,
            webdav,
            image_cache,
        }
    }

    pub fn with_img2img(
        text2img: Box<dyn ImageProvider>,
        img2img: Box<dyn ImageProvider>,
        default_quality: String,
        default_output_format: String,
        default_num_images: u32,
        default_image_size: String,
        default_image_size_tier: String,
        webdav: WebDavClient,
        image_cache: Arc<ImageCache>,
    ) -> Self {
        Self {
            provider: text2img,
            edit_provider: Some(img2img),
            default_quality,
            default_output_format,
            default_num_images,
            default_image_size,
            default_image_size_tier,
            webdav,
            image_cache,
        }
    }

    async fn upload_data_uri(&self, data_uri: &str) -> Result<String> {
        let after_data = data_uri
            .strip_prefix("data:")
            .ok_or_else(|| RockBotError::ToolCallParse("Invalid data URI".into()))?;
        let (mime_part, b64) = after_data
            .split_once(";base64,")
            .ok_or_else(|| RockBotError::ToolCallParse("Data URI missing ;base64,".into()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| RockBotError::ToolCallParse(format!("Base64 decode failed: {e}")))?;
        self.provider.upload_file(&bytes, mime_part).await
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
        "Generate or edit an image. For text-to-image, provide a prompt \
         and optional image_size. To edit or transform an image, the user's \
         attachments are automatically provided as image_urls — just describe \
         what to do in the prompt. \
         Returns a JSON object: {\"ok\": true, \"image_key\": \"...\", \"webdav_path\": \"...\"}. \
         Always share the image with the user in markdown image format \
         as `![{description}]({image_key})` so they can view the image inline. \
         After a successful image_gen call, respond to the user — do not call image_gen again."
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
        let t_start = std::time::Instant::now();
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
        params.quality = Some(self.default_quality.clone());
        params.output_format = Some(self.default_output_format.clone());
        params.num_images = Some(self.default_num_images);
        params.image_size = Some(ImageSizeValue::Preset(
            self.default_image_size.clone(),
        ));
        params.size_tier = Some(self.default_image_size_tier.clone());

        if let Some(image_urls) = args.get("image_urls").and_then(|v| v.as_array()) {
            let mut urls: Vec<String> = Vec::with_capacity(image_urls.len());
            for v in image_urls {
                let raw = v.as_str().map(String::from);
                match raw.as_deref() {
                    Some(uri) if uri.starts_with("data:") => {
                        if let Ok(uploaded_url) = self.upload_data_uri(uri).await {
                            debug!("Uploaded image to provider storage: {}", uploaded_url);
                            urls.push(uploaded_url);
                        } else {
                            warn!("Failed to upload data URI to provider storage, skipping it");
                        }
                    }
                    Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
                        urls.push(s.to_string());
                    }
                    Some(_) => {
                        debug!("Skipping non-URL image_urls entry");
                    }
                    None => {}
                }
            }
            if !urls.is_empty() {
                params.image_urls = Some(urls);
            }
        }

        let ext = ext_from_output_format(params.output_format.as_deref());

        let is_img2img = params.image_urls.is_some();
        let provider: &dyn ImageProvider = if is_img2img {
            self.edit_provider.as_deref().unwrap_or(self.provider.as_ref())
        } else {
            self.provider.as_ref()
        };

        debug!(
            "image_gen params: provider={} model={} img2img={} num_images={} quality={:?} output_format={:?} image_size={:?} image_urls_count={} prompt_len={} room={}",
            provider.provider_name(),
            provider.model_id(),
            is_img2img,
            params.num_images.unwrap_or(1),
            params.quality,
            params.output_format,
            params.image_size,
            params.image_urls.as_ref().map(|u| u.len()).unwrap_or(0),
            prompt.len(),
            room_id,
        );

        let image_bytes = provider.generate_image(&params).await.map_err(|e| {
            warn!("image_gen: generate_image failed: {e}");
            e
        })?;
        info!(
            "Image generated ({}): {} bytes elapsed_ms={}",
            provider.provider_name(),
            image_bytes.len(),
            t_start.elapsed().as_millis(),
        );

        let webdav_path = self.upload_to_webdav(webdav_dir, ext, image_bytes.clone()).await.map_err(|e| {
            warn!("image_gen: upload_to_webdav failed: {e}");
            e
        })?;
        info!(
            "Uploaded image to WebDAV: {} elapsed_ms={}",
            webdav_path,
            t_start.elapsed().as_millis(),
        );

        let share_url = self.webdav.create_nextcloud_share_link(&webdav_path).await;

        let mime = format!(
            "image/{}",
            ext.replace("jpg", "jpeg")
        );

        let image_key = args
            .get("image_cache_key")
            .and_then(|k| k.as_str())
            .map(String::from)
            .unwrap_or_else(uuid_string);

        self.image_cache.store(
            &image_key,
            GeneratedImage {
                webdav_path: webdav_path.clone(),
                image_bytes: image_bytes.clone(),
                mime_type: mime,
                share_url,
            },
        );

        info!(
            "image_gen total elapsed_ms={}",
            t_start.elapsed().as_millis(),
        );

        Ok(serde_json::json!({
            "ok": true,
            "webdav_path": webdav_path,
            "image_key": image_key,
        })
        .to_string())
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

    fn make_fal_provider() -> Box<dyn ImageProvider> {
        use crate::provider::FalAiProvider;
        let config = make_fal_config();
        Box::new(FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap())
    }

    fn make_webdav() -> WebDavClient {
        webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap()
    }

    fn make_image_cache() -> Arc<ImageCache> {
        Arc::new(ImageCache::new())
    }

    #[test]
    fn test_image_gen_tool_definition() {
        let tool = ImageGenTool::new(make_fal_provider(), "medium".into(), "png".into(), 1, "portrait_2_3".into(), "4K".into(), make_webdav(), make_image_cache());

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
        assert!(params["properties"].get("image_size").is_none(), "image_size hidden from LLM — set via config");
        assert!(params["properties"].get("image_urls").is_some());
    }

    #[tokio::test]
    async fn test_execute_missing_prompt() {
        let tool = ImageGenTool::new(make_fal_provider(), "medium".into(), "png".into(), 1, "portrait_2_3".into(), "4K".into(), make_webdav(), make_image_cache());
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let tool = ImageGenTool::new(make_fal_provider(), "medium".into(), "png".into(), 1, "portrait_2_3".into(), "4K".into(), make_webdav(), make_image_cache());
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
            "image_size": "landscape_16_9"
        }"#).unwrap();

        let mut params = ImageGenParams::new(args["prompt"].as_str().unwrap());
        params.quality = Some("medium".into());
        params.output_format = Some("png".into());
        params.num_images = Some(1);
        if let Some(size_val) = args.get("image_size") {
            params.image_size = size_val.as_str().map(|s| ImageSizeValue::Preset(s.to_string()));
        }

        assert_eq!(params.quality.as_deref(), Some("medium"));
        assert_eq!(params.output_format.as_deref(), Some("png"));
        assert_eq!(params.num_images, Some(1));

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
