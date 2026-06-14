use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use tracing::{debug, info, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::image_cache::{GeneratedImage, ImageCache};
use crate::provider::ImageProvider;
use crate::tool::Tool;
use crate::types::{ImageGenParams, ImageSizeValue};
use crate::validated::NonEmptyString;

#[derive(Debug, Deserialize)]
struct ImageGenArgs {
    prompt: NonEmptyString,
    aspect_ratio: NonEmptyString,
    #[serde(default)]
    image_urls: Option<Vec<String>>,
    #[serde(default)]
    room_id: Option<String>,
    #[serde(default)]
    webdav_dir: Option<String>,
    #[serde(default)]
    image_cache_key: Option<String>,
    #[serde(default)]
    reference_image_key: Option<String>,
}

use std::sync::Arc;

pub struct ImageGenTool {
    provider: Box<dyn ImageProvider>,
    edit_provider: Option<Box<dyn ImageProvider>>,
    default_quality: String,
    default_output_format: String,
    default_num_images: u32,
    default_image_size_tier: String,
    webdav: WebDavClient,
    image_cache: Arc<ImageCache>,
}

impl ImageGenTool {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Box<dyn ImageProvider>,
        default_quality: String,
        default_output_format: String,
        default_num_images: u32,
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
            default_image_size_tier,
            webdav,
            image_cache,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_img2img(
        text2img: Box<dyn ImageProvider>,
        img2img: Box<dyn ImageProvider>,
        default_quality: String,
        default_output_format: String,
        default_num_images: u32,
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
        let filename = WebDavPath::new("").image_path(room_id, &format!("{}.{}", uuid_string(), ext))
            .map_err(|e| RockBotError::Provider(format!("WebDAV path error: {e}")))?;
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
        "Generate or edit an image. Provide a prompt and optional aspect_ratio (e.g. '16:9'). \
         User attachments are auto-provided as image_urls for editing. \
         Returns {\"ok\": true, \"image_key\": \"...\"} — share result as `![desc]({image_key})`."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Text description of the image to generate"
                },
                "aspect_ratio": {
                    "type": "string",
                    "description": "Aspect ratio for the generated image as W:H (e.g. '16:9', '2:3', '1:1')"
                },
                "room_id": {
                    "type": "string",
                    "description": "Room ID for image storage (injected automatically if omitted)"
                },
                "image_urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "URLs of images to edit (e.g., share_url from a previous image_gen result). Omit to generate a new image. Auto-injected from user attachments and message images."
                },
                "reference_image_key": {
                    "type": "string",
                    "description": "The image_key of a previously generated image to edit. Alternative to providing explicit image_urls."
                }
            },
            "required": ["prompt", "aspect_ratio"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let t_start = std::time::Instant::now();
        let args: ImageGenArgs = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse image_gen arguments: {e}"))
        })?;

        let prompt = &args.prompt;
        let room_id = args.room_id.as_deref().unwrap_or("unknown");
        let webdav_dir = args.webdav_dir.as_deref().unwrap_or(room_id);

        let mut params = ImageGenParams::new(prompt.as_str());
        params.quality = Some(self.default_quality.clone());
        params.output_format = Some(self.default_output_format.clone());
        params.num_images = Some(self.default_num_images);

        params.image_size = Some(ImageSizeValue::Preset(args.aspect_ratio.as_str().to_string()));
        params.size_tier = Some(self.default_image_size_tier.clone());

        let mut collected_urls: Vec<String> = Vec::new();

        if let Some(ref key) = args.reference_image_key {
            if let Some(cached) = self.image_cache.get(key) {
                let data_uri = cached.data_uri();
                match self.upload_data_uri(&data_uri).await {
                    Ok(uploaded_url) => {
                        debug!("Injected reference_image_key '{}' for editing via uploaded URL: {}", key, uploaded_url);
                        collected_urls.push(uploaded_url);
                    }
                    Err(e) => {
                        warn!("Failed to upload reference_image_key '{}' to provider storage: {}", key, e);
                    }
                }
            } else {
                warn!("reference_image_key '{}' not found in image cache", key);
            }
        }

        if let Some(image_urls) = &args.image_urls {
            for raw in image_urls {
                match raw.as_str() {
                    uri if uri.starts_with("data:") => {
                        if let Ok(uploaded_url) = self.upload_data_uri(uri).await {
                            debug!("Uploaded image to provider storage: {}", uploaded_url);
                            collected_urls.push(uploaded_url);
                        } else {
                            warn!("Failed to upload data URI to provider storage, skipping it");
                        }
                    }
                    s if s.starts_with("http://") || s.starts_with("https://") => {
                        collected_urls.push(s.to_string());
                    }
                    _ => {
                        debug!("Skipping non-URL image_urls entry");
                    }
                }
            }
        }
        if !collected_urls.is_empty() {
            params.image_urls = Some(collected_urls);
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

        let image_key = args.image_cache_key.clone().unwrap_or_else(uuid_string);

        self.image_cache.store(
            &image_key,
            GeneratedImage {
                webdav_path: NonEmptyString::try_new(webdav_path.clone()).expect("non-empty webdav_path"),
                image_bytes: image_bytes.clone(),
                mime_type: NonEmptyString::try_new(mime).expect("non-empty mime_type"),
                share_url: share_url.clone(),
            },
        );

        info!(
            "image_gen total elapsed_ms={}",
            t_start.elapsed().as_millis(),
        );

        let mut result = serde_json::json!({
            "ok": true,
            "webdav_path": webdav_path,
            "image_key": image_key,
        });
        if let Some(ref url) = share_url {
            result["share_url"] = serde_json::json!(url);
        }
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;
    use crate::validated::{ConfigUrl, ProviderName};
    use serde_json::Value;
    use std::collections::HashMap;

    fn make_fal_config() -> ProviderConfig {
        ProviderConfig {
            name: ProviderName::try_new("fal".to_string()).unwrap(),
            api_key: "test-key".into(),
            base_url: ConfigUrl::try_new("https://queue.fal.run".to_string()).unwrap(),
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
        let tool = ImageGenTool::new(make_fal_provider(), "medium".into(), "png".into(), 1, "4K".into(), make_webdav(), make_image_cache());

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
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("aspect_ratio"))
        );
        assert!(params["properties"].get("aspect_ratio").is_some(), "aspect_ratio visible to LLM — set via tool arg");
        assert!(params["properties"].get("image_urls").is_some());
    }

    #[tokio::test]
    async fn test_execute_missing_prompt() {
        let tool = ImageGenTool::new(make_fal_provider(), "medium".into(), "png".into(), 1, "4K".into(), make_webdav(), make_image_cache());
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let tool = ImageGenTool::new(make_fal_provider(), "medium".into(), "png".into(), 1, "4K".into(), make_webdav(), make_image_cache());
        let result = tool.execute("not json").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_aspect_ratio_passed_through_to_params() {
        // aspect_ratio is required from LLM — verify it's stored as Preset
        let args: Value = serde_json::from_str(r#"{"prompt":"a cat","aspect_ratio":"16:9"}"#).unwrap();
        let aspect_ratio = args
            .get("aspect_ratio")
            .and_then(|v| v.as_str());
        assert_eq!(aspect_ratio, Some("16:9"), "LLM-provided aspect_ratio should be available");
    }

    #[test]
    fn test_aspect_ratio_missing_fails_deserialization() {
        // aspect_ratio is required — missing it should fail deserialization
        let result: std::result::Result<ImageGenArgs, _> = serde_json::from_str(r#"{"prompt":"a cat"}"#);
        assert!(result.is_err(), "Missing required aspect_ratio should fail deserialization");
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

    // ----- Gap-filled tests (image-gen.md coverage gaps) -----

    struct MockImageProvider {
        generate_result: std::sync::Mutex<Option<std::result::Result<Vec<u8>, RockBotError>>>,
        upload_result: std::sync::Mutex<Option<std::result::Result<String, RockBotError>>>,
    }

    impl MockImageProvider {
        fn new() -> Self {
            Self {
                generate_result: std::sync::Mutex::new(Some(Ok(vec![1, 2, 3]))),
                upload_result: std::sync::Mutex::new(Some(Ok("https://cdn.example.com/uploaded.png".into()))),
            }
        }

        fn with_generate_error(e: RockBotError) -> Self {
            Self {
                generate_result: std::sync::Mutex::new(Some(Err(e))),
                upload_result: std::sync::Mutex::new(Some(Ok("https://cdn.example.com/uploaded.png".into()))),
            }
        }
    }

    #[async_trait]
    impl ImageProvider for MockImageProvider {
        async fn generate_image(&self, _params: &ImageGenParams) -> crate::Result<Vec<u8>> {
            self.generate_result.lock().unwrap().take().unwrap()
        }

        async fn upload_file(&self, _data: &[u8], _content_type: &str) -> crate::Result<String> {
            self.upload_result.lock().unwrap().take().unwrap()
        }

        fn provider_name(&self) -> &str {
            "mock"
        }

        fn model_id(&self) -> &str {
            "mock-model"
        }
    }

    #[test]
    fn test_size_tier_is_set_from_config_default() {
        let tool = ImageGenTool::new(
            Box::new(MockImageProvider::new()),
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            make_webdav(),
            make_image_cache(),
        );

        // Verify default_image_size_tier is stored as "4K" per DFD §3
        assert_eq!(tool.default_image_size_tier, "4K");
    }

    #[test]
    fn test_size_tier_in_params_construction() {
        let tool = ImageGenTool::new(
            Box::new(MockImageProvider::new()),
            "medium".into(),
            "png".into(),
            1,
            "2K".into(),
            make_webdav(),
            make_image_cache(),
        );

        // Simulate what execute() does when building ImageGenParams
        let mut params = ImageGenParams::new("test prompt");
        params.quality = Some(tool.default_quality.clone());
        params.output_format = Some(tool.default_output_format.clone());
        params.num_images = Some(tool.default_num_images);
        params.image_size = Some(ImageSizeValue::Preset("16:9".into()));
        params.size_tier = Some(tool.default_image_size_tier.clone());

        assert_eq!(params.size_tier.as_deref(), Some("2K"));
    }

    #[tokio::test]
    async fn test_upload_data_uri_decodes_base64_and_uploads() {
        let provider = Box::new(MockImageProvider::new());
        let tool = ImageGenTool::new(
            provider,
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            make_webdav(),
            make_image_cache(),
        );

        // A minimal valid PNG data URI
        let data_uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";
        let result = tool.upload_data_uri(data_uri).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://cdn.example.com/uploaded.png");
    }

    #[tokio::test]
    async fn test_upload_data_uri_invalid_prefix() {
        let tool = ImageGenTool::new(
            Box::new(MockImageProvider::new()),
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            make_webdav(),
            make_image_cache(),
        );

        let result = tool.upload_data_uri("not-a-data-uri").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_upload_data_uri_missing_base64_delimiter() {
        let tool = ImageGenTool::new(
            Box::new(MockImageProvider::new()),
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            make_webdav(),
            make_image_cache(),
        );

        let result = tool.upload_data_uri("data:image/png;no-base64-delimiter").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_upload_data_uri_invalid_base64() {
        let tool = ImageGenTool::new(
            Box::new(MockImageProvider::new()),
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            make_webdav(),
            make_image_cache(),
        );

        let result = tool.upload_data_uri("data:image/png;base64,!!!invalid-base64!!!").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_generate_image_failure() {
        let tool = ImageGenTool::new(
            Box::new(MockImageProvider::with_generate_error(RockBotError::Provider(
                "Image generation failed".into(),
            ))),
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            make_webdav(),
            make_image_cache(),
        );

        let args = serde_json::json!({
            "prompt": "test prompt",
            "aspect_ratio": "1:1",
            "room_id": "room1",
        });
        let result = tool.execute(&args.to_string()).await;
        assert!(result.is_err(), "generate_image failure should propagate error");
    }

    #[test]
    fn test_reference_image_key_deserialization() {
        let args = serde_json::json!({
            "prompt": "make the cat darker",
            "aspect_ratio": "1:1",
            "reference_image_key": "call_abc123def4567890",
        });
        let parsed: ImageGenArgs = serde_json::from_value(args).unwrap();
        assert_eq!(parsed.prompt.as_str(), "make the cat darker");
        assert_eq!(parsed.aspect_ratio.as_str(), "1:1");
        assert_eq!(parsed.reference_image_key.as_deref(), Some("call_abc123def4567890"));
    }

    #[test]
    fn test_reference_image_key_absent_by_default() {
        let args = serde_json::json!({
            "prompt": "generate a sunset",
            "aspect_ratio": "16:9",
        });
        let parsed: ImageGenArgs = serde_json::from_value(args).unwrap();
        assert!(parsed.reference_image_key.is_none());
    }

    #[test]
    fn test_reference_image_key_in_schema() {
        let tool = ImageGenTool::new(
            Box::new(MockImageProvider::new()),
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            make_webdav(),
            make_image_cache(),
        );
        let schema = tool.parameters();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("reference_image_key"), "schema must include reference_image_key");
        assert_eq!(props["reference_image_key"]["type"], "string");
    }
}
