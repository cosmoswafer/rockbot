use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::image_cache::{GeneratedImage, ImageCache};
use crate::provider::ImageProvider;
use crate::tool::Tool;
use crate::types::{ImageGenParams, ImageModelCatalog, ImageModelEntry, ImageSizeValue};
use crate::validated::{ModelAlias, NonEmptyString};

#[derive(Debug, Deserialize)]
struct ImageGenArgs {
    prompt: NonEmptyString,
    aspect_ratio: NonEmptyString,
    #[serde(default)]
    model: Option<ModelAlias>,
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

/// Provider backends (t2i + optional edit) of one `[[image_providers]]`
/// entry, keyed by the entry's name — the tool routes each resolved alias to
/// the backend of its owning provider (issue #96).
pub struct ImageBackend {
    pub t2i: Box<dyn ImageProvider>,
    pub edit: Option<Box<dyn ImageProvider>>,
}

impl ImageBackend {
    pub fn new(t2i: Box<dyn ImageProvider>, edit: Option<Box<dyn ImageProvider>>) -> Self {
        Self { t2i, edit }
    }

    fn select(&self, is_img2img: bool) -> &dyn ImageProvider {
        if is_img2img {
            self.edit.as_deref().unwrap_or(self.t2i.as_ref())
        } else {
            self.t2i.as_ref()
        }
    }
}

pub struct ImageGenTool {
    backends: HashMap<String, ImageBackend>,
    default_backend: String,
    model_catalog: ImageModelCatalog,
    description: String,
    default_quality: String,
    default_output_format: String,
    default_num_images: u32,
    default_image_size_tier: String,
    #[allow(dead_code)]
    default_image_size: Option<String>,
    default_enable_safety_checker: bool,
    webdav: WebDavClient,
    image_cache: Arc<ImageCache>,
}

impl ImageGenTool {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backends: HashMap<String, ImageBackend>,
        default_backend: String,
        model_catalog: ImageModelCatalog,
        default_quality: String,
        default_output_format: String,
        default_num_images: u32,
        default_image_size_tier: String,
        default_enable_safety_checker: bool,
        webdav: WebDavClient,
        image_cache: Arc<ImageCache>,
    ) -> Self {
        let description = build_description(&model_catalog);
        Self {
            backends,
            default_backend,
            model_catalog,
            description,
            default_quality,
            default_output_format,
            default_num_images,
            default_image_size_tier,
            default_image_size: None,
            default_enable_safety_checker,
            webdav,
            image_cache,
        }
    }

    /// `aspect_ratio` parameter description — the auto-dimensional hint is
    /// derived from the catalog, so it appears only while a seedream v5 model
    /// is actually configured.
    fn aspect_ratio_description(&self) -> String {
        let mut desc = String::from(
            "Aspect ratio: '16:9', '2:3', '1:1', '4:3', '3:4', '3:2' as W:H.",
        );
        if self.model_catalog.supports_auto_aspect() {
            desc.push_str(" Seedream5 (Fal) also accepts 'auto_2K' or 'auto_1K' to auto-select dimensions.");
        }
        desc
    }

    async fn upload_data_uri(&self, provider: &dyn ImageProvider, data_uri: &str) -> Result<String> {
        let after_data = data_uri
            .strip_prefix("data:")
            .ok_or_else(|| RockBotError::ToolCallParse("Invalid data URI".into()))?;
        let (mime_part, b64) = after_data
            .split_once(";base64,")
            .ok_or_else(|| RockBotError::ToolCallParse("Data URI missing ;base64,".into()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| RockBotError::ToolCallParse(format!("Base64 decode failed: {e}")))?;
        provider.upload_file(&bytes, mime_part).await
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

/// Derives the LLM-facing tool description from the catalog at registry time
/// (issue #95). Config is the single source of truth — model names, defaults,
/// and the auto-dimensional aspect hint all come from `ImageModelCatalog`, so
/// the text can never drift from `[image_providers]` / `[image_model]`.
fn build_description(catalog: &ImageModelCatalog) -> String {
    let models = if catalog.is_empty() {
        "(no image models configured)".to_string()
    } else {
        catalog
            .allowed_aliases()
            .iter()
            .filter_map(|alias| {
                catalog.entry(alias).map(|e| {
                    let base = format!("{} ({}, {}", alias, e.model_id, e.provider_name);
                    match &e.edit_model_id {
                        Some(id) => format!("{base}, edit:{id})"),
                        None => format!("{base})"),
                    }
                })
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut desc = format!(
        "Generate or edit an image. Provide a prompt and required aspect_ratio. \
         Available image models: {models}. \
         Default: '{}'. Editing passes the input images to the same alias — \
         dedicated edit endpoints are selected automatically when available. \
         Standard ratios: '16:9', '2:3', '1:1', '4:3', '3:4', '3:2'. \
         User attachments are auto-provided as image_urls for editing. \
         Returns {{\"ok\": true, \"image_key\": \"...\"}} — share result as `![desc]({{image_key}})`.",
        catalog.default_alias(),
    );
    if catalog.supports_auto_aspect() {
        desc.push_str(" Seedream5 models also accept 'auto_2K' or 'auto_1K' to auto-select dimensions.");
    }
    desc
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
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        let available = if self.model_catalog.is_empty() {
            "no models configured".to_string()
        } else {
            self.model_catalog.valid_alias_list()
        };
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Text description of the image to generate"
                },
                "aspect_ratio": {
                    "type": "string",
                    "description": self.aspect_ratio_description(),
                },
                "model": {
                    "type": "string",
                    "description": format!(
                        "Image model alias (from [image_providers] models). Default: '{}'. Editing reuses the same alias. Available: {}",
                        self.model_catalog.default_alias(),
                        available
                    )
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
        });
        let aliases: Vec<serde_json::Value> = self
            .model_catalog
            .allowed_aliases()
            .into_iter()
            .map(|a| serde_json::Value::String(a.to_string()))
            .collect();
        if !aliases.is_empty() {
            schema["properties"]["model"]["enum"] = serde_json::Value::Array(aliases);
        }
        schema
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

        // Model alias → owning provider entry via the catalog (default alias
        // when omitted). The backend is picked first; the exact model id is
        // resolved mode-aware AFTER input images are collected — edit-mode
        // calls swap to the entry's dedicated edit endpoint when one exists
        // (issue #100).
        let backend_for =
            |provider: &str| -> Option<&ImageBackend> { self.backends.get(provider) };

        let (backend, effective): (&ImageBackend, Option<ImageModelEntry>) = match &args.model {
            Some(alias) => {
                let e = self.model_catalog.entry(alias.as_str()).ok_or_else(|| {
                    RockBotError::ToolCallParse(format!(
                        "image_gen: model alias '{}' not in image model catalog — valid aliases: {}",
                        alias.as_str(),
                        self.model_catalog.valid_alias_list()
                    ))
                })?;
                let b = backend_for(&e.provider_name).ok_or_else(|| {
                    RockBotError::ToolCallParse(format!(
                        "image_gen: model alias '{}' routes to provider '{}' which has no available backend",
                        alias.as_str(),
                        e.provider_name
                    ))
                })?;
                (b, Some(e.clone()))
            }
            None => {
                let b = backend_for(&self.default_backend).ok_or_else(|| {
                    RockBotError::Provider(format!(
                        "image_gen: default image provider '{}' has no available backend",
                        self.default_backend
                    ))
                })?;
                // Lenient default: adopt the default alias's entry only when
                // it belongs to the default provider (config leniency — an
                // unresolvable/mismatched default keeps the baked backend
                // model and never blocks tool use).
                let e = self
                    .model_catalog
                    .entry(self.model_catalog.default_alias())
                    .filter(|e| e.provider_name == self.default_backend)
                    .cloned();
                (b, e)
            }
        };

        params.image_size = Some(ImageSizeValue::Preset(args.aspect_ratio.as_str().to_string()));
        params.size_tier = Some(self.default_image_size_tier.clone());
        params.enable_safety_checker = Some(self.default_enable_safety_checker);

        let mut collected_urls: Vec<String> = Vec::new();

        if let Some(ref key) = args.reference_image_key {
            if let Some(cached) = self.image_cache.get(key) {
                let data_uri = cached.data_uri();
                match self.upload_data_uri(backend.t2i.as_ref(), &data_uri).await {
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
                        if let Ok(uploaded_url) = self.upload_data_uri(backend.t2i.as_ref(), uri).await {
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

        // Mode-aware model id (issue #100): dedicated edit endpoint when the
        // call edits images and the alias carries an edit companion; the plain
        // model id otherwise (same-model providers).
        if let Some(e) = &effective {
            params.model_id = Some(e.model_id_for_mode(is_img2img).to_string());
        }

        let provider: &dyn ImageProvider = backend.select(is_img2img);

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
    use crate::types::ImageModelEntry;
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
            edit_models: HashMap::new(),
        }
    }

    fn make_fal_provider() -> Box<dyn ImageProvider> {
        use crate::provider::FalAiProvider;
        let config = make_fal_config();
        Box::new(FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap())
    }

    fn entry(alias: &str, model_id: &str, edit: Option<&str>, provider: &str) -> ImageModelEntry {
        ImageModelEntry {
            alias: alias.to_string(),
            model_id: model_id.to_string(),
            edit_model_id: edit.map(|s| s.to_string()),
            provider_name: provider.to_string(),
        }
    }

    fn make_model_catalog() -> ImageModelCatalog {
        ImageModelCatalog::new(
            vec![entry("flux", "fal-ai/flux/schnell", None, "fal")],
            "flux",
        )
    }

    /// Single-fal-backend tool with standard defaults — most unit tests use
    /// this shape; multi-provider tests build the map manually.
    fn make_tool(provider: Box<dyn ImageProvider>, catalog: ImageModelCatalog) -> ImageGenTool {
        ImageGenTool::new(
            HashMap::from([("fal".to_string(), ImageBackend::new(provider, None))]),
            "fal".to_string(),
            catalog,
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            false,
            make_webdav(),
            make_image_cache(),
        )
    }

    fn make_webdav() -> WebDavClient {
        webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap()
    }

    fn make_image_cache() -> Arc<ImageCache> {
        Arc::new(ImageCache::new())
    }

    #[test]
    fn test_image_gen_tool_definition() {
        let tool = make_tool(make_fal_provider(), make_model_catalog());

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
        assert!(params["properties"].get("model").is_some(), "model alias visible to LLM for per-call selection");
        assert!(params["properties"].get("image_urls").is_some());
    }

    #[tokio::test]
    async fn test_execute_missing_prompt() {
        let tool = make_tool(make_fal_provider(), make_model_catalog());
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let tool = make_tool(make_fal_provider(), make_model_catalog());
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
        params_sink: std::sync::Arc<std::sync::Mutex<Option<ImageGenParams>>>,
    }

    impl MockImageProvider {
        fn new() -> Self {
            Self {
                generate_result: std::sync::Mutex::new(Some(Ok(vec![1, 2, 3]))),
                upload_result: std::sync::Mutex::new(Some(Ok("https://cdn.example.com/uploaded.png".into()))),
                params_sink: std::sync::Arc::new(std::sync::Mutex::new(None)),
            }
        }

        fn with_generate_error(e: RockBotError) -> Self {
            Self {
                generate_result: std::sync::Mutex::new(Some(Err(e))),
                upload_result: std::sync::Mutex::new(Some(Ok("https://cdn.example.com/uploaded.png".into()))),
                params_sink: std::sync::Arc::new(std::sync::Mutex::new(None)),
            }
        }
    }

    #[async_trait]
    impl ImageProvider for MockImageProvider {
        async fn generate_image(&self, params: &ImageGenParams) -> crate::Result<Vec<u8>> {
            *self.params_sink.lock().unwrap() = Some(params.clone());
            match self.generate_result.lock().unwrap().as_ref() {
                Some(Ok(bytes)) => Ok(bytes.clone()),
                Some(Err(e)) => Err(RockBotError::Provider(format!("mock: {e}"))),
                None => Err(RockBotError::Provider("capture-only".into())),
            }
        }

        async fn upload_file(&self, _data: &[u8], _content_type: &str) -> crate::Result<String> {
            match self.upload_result.lock().unwrap().as_ref() {
                Some(Ok(url)) => Ok(url.clone()),
                Some(Err(e)) => Err(RockBotError::Provider(format!("mock: {e}"))),
                None => Err(RockBotError::Provider("upload not stubbed".into())),
            }
        }

        fn provider_name(&self) -> &str {
            "mock"
        }

        fn model_id(&self) -> &str {
            "mock-model"
        }
    }

    /// Provider that records every `ImageGenParams` it receives, then errors —
    /// used to assert what `execute()` sends downstream.
    fn make_recording_provider() -> (Box<dyn ImageProvider>, std::sync::Arc<std::sync::Mutex<Option<ImageGenParams>>>) {
        let sink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let provider = MockImageProvider {
            generate_result: std::sync::Mutex::new(Some(Err(RockBotError::Provider(
                "capture-only".into(),
            )))),
            upload_result: std::sync::Mutex::new(Some(Ok("https://cdn.example.com/uploaded.png".into()))),
            params_sink: sink.clone(),
        };
        (Box::new(provider), sink)
    }

    #[test]
    fn test_size_tier_is_set_from_config_default() {
        let tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());

        // Verify default_image_size_tier is stored as "4K" per DFD §3
        assert_eq!(tool.default_image_size_tier, "4K");
    }

    #[test]
    fn test_size_tier_in_params_construction() {
        let tool = ImageGenTool::new(
            HashMap::from([("fal".to_string(), ImageBackend::new(Box::new(MockImageProvider::new()), None))]),
            "fal".to_string(),
            make_model_catalog(),
            "medium".into(),
            "png".into(),
            1,
            "2K".into(),
            false,
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
        let tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());
        let provider = tool.backends["fal"].t2i.as_ref();

        // A minimal valid PNG data URI
        let data_uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";
        let result = tool.upload_data_uri(provider, data_uri).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://cdn.example.com/uploaded.png");
    }

    #[tokio::test]
    async fn test_upload_data_uri_invalid_prefix() {
        let tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());

        let result = tool.upload_data_uri(tool.backends["fal"].t2i.as_ref(), "not-a-data-uri").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_upload_data_uri_missing_base64_delimiter() {
        let tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());

        let result = tool.upload_data_uri(tool.backends["fal"].t2i.as_ref(), "data:image/png;no-base64-delimiter").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_upload_data_uri_invalid_base64() {
        let tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());

        let result = tool.upload_data_uri(tool.backends["fal"].t2i.as_ref(), "data:image/png;base64,!!!invalid-base64!!!").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_generate_image_failure() {
        let tool = make_tool(
            Box::new(MockImageProvider::with_generate_error(RockBotError::Provider(
                "Image generation failed".into(),
            ))),
            make_model_catalog(),
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
        let tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());
        let schema = tool.parameters();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("reference_image_key"), "schema must include reference_image_key");
        assert_eq!(props["reference_image_key"]["type"], "string");
    }

    // ----- Per-call model selection (issue #92) -----

    fn make_multi_model_catalog() -> ImageModelCatalog {
        ImageModelCatalog::new(
            vec![
                entry(
                    "seedream5",
                    "bytedance/seedream/v5/pro/text-to-image",
                    Some("bytedance/seedream/v5/pro/edit"),
                    "fal",
                ),
                entry("mai2pro", "microsoft/mai-image-2.5-pro", None, "openrouter"),
            ],
            "mai2pro",
        )
    }

    #[tokio::test]
    async fn test_execute_model_alias_override_reaches_params() {
        let (provider, sink) = make_recording_provider();
        let tool = make_tool(provider, make_multi_model_catalog());
        let args = serde_json::json!({
            "prompt": "a cat",
            "aspect_ratio": "1:1",
            "model": "seedream5",
        });
        let _ = tool.execute(&args.to_string()).await;
        let guard = sink.lock().unwrap();
        let captured = guard.as_ref().expect("provider received params");
        assert_eq!(
            captured.model_id.as_deref(),
            Some("bytedance/seedream/v5/pro/text-to-image"),
            "model alias must resolve to the catalog's model id"
        );
    }

    #[tokio::test]
    async fn test_execute_model_alias_omitted_leaves_params_model_id_none() {
        let (provider, sink) = make_recording_provider();
        let tool = make_tool(provider, make_multi_model_catalog());
        let args = serde_json::json!({
            "prompt": "a cat",
            "aspect_ratio": "1:1",
        });
        let _ = tool.execute(&args.to_string()).await;
        let guard = sink.lock().unwrap();
        let captured = guard.as_ref().expect("provider received params");
        assert!(captured.model_id.is_none(), "omitted model alias must keep provider's configured default");
    }

    #[tokio::test]
    async fn test_execute_unknown_model_alias_rejected_at_parse() {
        let tool = make_tool(
            Box::new(MockImageProvider::with_generate_error(RockBotError::Provider("x".into()))),
            make_multi_model_catalog(),
        );
        let args = serde_json::json!({
            "prompt": "a cat",
            "aspect_ratio": "1:1",
            "model": "nope",
        });
        let err = tool.execute(&args.to_string()).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not in image model catalog"), "err: {msg}");
        assert!(msg.contains("mai2pro") && msg.contains("seedream5"), "err should list valid aliases: {msg}");
    }

    // ----- Per-provider backend routing (issue #96) -----

    #[tokio::test]
    async fn test_execute_routes_model_alias_to_its_own_provider_backend() {
        let (fal_provider, fal_sink) = make_recording_provider();
        let (or_provider, or_sink) = make_recording_provider();
        let tool = ImageGenTool::new(
            HashMap::from([
                ("fal".to_string(), ImageBackend::new(fal_provider, None)),
                ("openrouter".to_string(), ImageBackend::new(or_provider, None)),
            ]),
            "openrouter".to_string(),
            make_multi_model_catalog(),
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            false,
            make_webdav(),
            make_image_cache(),
        );

        // seedream5 → fal backend
        let _ = tool.execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"seedream5"}"#).await;
        assert_eq!(
            fal_sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("bytedance/seedream/v5/pro/text-to-image"),
            "fal-tagged alias must hit the fal backend"
        );
        assert!(or_sink.lock().unwrap().is_none(), "fal alias must not hit openrouter");

        // mai2pro → openrouter backend
        let _ = tool.execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"mai2pro"}"#).await;
        assert_eq!(
            or_sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("microsoft/mai-image-2.5-pro"),
            "openrouter-tagged alias must hit the openrouter backend"
        );

        // omitted model → default backend (openrouter); default alias 'mai2pro'
        // belongs to openrouter, so its entry now resolves mode-aware and sets
        // the t2i id explicitly
        let _ = tool.execute(r#"{"prompt":"a cat","aspect_ratio":"1:1"}"#).await;
        let or_captured = or_sink.lock().unwrap();
        let captured = or_captured.as_ref().unwrap();
        assert_eq!(
            captured.model_id.as_deref(),
            Some("microsoft/mai-image-2.5-pro"),
            "omitted alias adopts the default alias entry (mode-aware)"
        );
        assert_eq!(captured.prompt.as_str(), "a cat");
    }

    #[tokio::test]
    async fn test_execute_edit_mode_swaps_to_dedicated_endpoint() {
        // Default alias = seedream5 (fal pair); tool default backend = fal.
        let catalog = ImageModelCatalog::new(
            vec![
                entry(
                    "seedream5",
                    "bytedance/seedream/v5/pro/text-to-image",
                    Some("bytedance/seedream/v5/pro/edit"),
                    "fal",
                ),
                entry("mai2pro", "microsoft/mai-image-2.5-pro", None, "openrouter"),
            ],
            "seedream5",
        );
        let (fal_provider, fal_sink) = make_recording_provider();
        let (or_provider, or_sink) = make_recording_provider();
        let tool = ImageGenTool::new(
            HashMap::from([
                ("fal".to_string(), ImageBackend::new(fal_provider, None)),
                ("openrouter".to_string(), ImageBackend::new(or_provider, None)),
            ]),
            "fal".to_string(),
            catalog,
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            false,
            make_webdav(),
            make_image_cache(),
        );

        // Edit call WITHOUT explicit model → default alias pair swaps to the
        // dedicated edit endpoint id (issue #100)
        let _ = tool.execute(r#"{"prompt":"recolor","aspect_ratio":"1:1","image_urls":["https://example.com/in.png"]}"#).await;
        assert_eq!(
            fal_sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("bytedance/seedream/v5/pro/edit"),
            "default paired alias must swap to its edit endpoint in edit mode"
        );

        // Same-model editing: mai2pro (no companion) keeps the plain id in edit mode
        let _ = tool.execute(r#"{"prompt":"recolor","aspect_ratio":"1:1","model":"mai2pro","image_urls":["https://example.com/in.png"]}"#).await;
        assert_eq!(
            or_sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("microsoft/mai-image-2.5-pro"),
            "alias without companion reuses the t2i id when editing (issue #100)"
        );
    }

    #[tokio::test]
    async fn test_execute_routing_requires_available_backend() {
        let tool = ImageGenTool::new(
            HashMap::from([(
                "fal".to_string(),
                ImageBackend::new(Box::new(MockImageProvider::new()), None),
            )]),
            "fal".to_string(),
            make_multi_model_catalog(), // mai2pro → openrouter, but no openrouter backend
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            false,
            make_webdav(),
            make_image_cache(),
        );
        let err = tool
            .execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"mai2pro"}"#)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no available backend") && msg.contains("openrouter"), "err: {msg}");
    }

    #[test]
    fn test_schema_model_enum_lists_catalog_aliases() {
        let tool = make_tool(Box::new(MockImageProvider::new()), make_multi_model_catalog());
        let schema = tool.parameters();
        let model = &schema["properties"]["model"];
        assert_eq!(model["type"], "string");
        assert_eq!(model["enum"], serde_json::json!(["mai2pro", "seedream5"]));
    }

    #[test]
    fn test_schema_model_has_no_enum_when_catalog_empty() {
        let tool = make_tool(
            Box::new(MockImageProvider::new()),
            ImageModelCatalog::new(Vec::new(), "mai2pro"),
        );
        let schema = tool.parameters();
        let model = &schema["properties"]["model"];
        assert!(model.get("enum").is_none(), "empty catalog must not emit an enum");
        assert!(model["description"].as_str().unwrap().contains("no models configured"));
    }

    // ----- Dynamic tool description from [image_providers] config (issue #95) -----

    #[test]
    fn test_tool_description_lists_models_and_defaults() {
        let tool = make_tool(Box::new(MockImageProvider::new()), make_multi_model_catalog());
        let desc = tool.description();
        assert!(desc.contains("Available image models: mai2pro (microsoft/mai-image-2.5-pro, openrouter), seedream5 (bytedance/seedream/v5/pro/text-to-image, fal, edit:bytedance/seedream/v5/pro/edit)"), "desc: {desc}");
        assert!(desc.contains("Default: 'mai2pro'"), "desc: {desc}");
        assert!(desc.contains("auto_2K") && desc.contains("auto_1K"), "seedream5 in catalog → auto hint: {desc}");
    }

    #[test]
    fn test_tool_description_omits_auto_hint_without_seedream() {
        let tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());
        let desc = tool.description();
        assert!(desc.contains("flux (fal-ai/flux/schnell, fal)"), "desc: {desc}");
        assert!(!desc.contains("auto_2K") && !desc.contains("auto_1K"), "no seedream → no auto hint: {desc}");
        assert!(desc.contains("Default: 'flux'"), "desc: {desc}");
    }

    #[test]
    fn test_tool_description_empty_catalog() {
        let tool = make_tool(
            Box::new(MockImageProvider::new()),
            ImageModelCatalog::new(Vec::new(), "mai2pro"),
        );
        let desc = tool.description();
        assert!(desc.contains("no image models configured"), "desc: {desc}");
        assert!(!desc.contains("auto_2K"), "no seedream → no auto hint: {desc}");
    }

    #[test]
    fn test_aspect_ratio_description_auto_hint_conditional() {
        let seedream_tool = make_tool(Box::new(MockImageProvider::new()), make_multi_model_catalog());
        let flux_tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());
        let seedream_desc = seedream_tool
            .parameters()["properties"]["aspect_ratio"]["description"]
            .as_str()
            .unwrap()
            .to_string();
        let flux_desc = flux_tool
            .parameters()["properties"]["aspect_ratio"]["description"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(seedream_desc.contains("auto_2K") && seedream_desc.contains("auto_1K"), "seedream_desc: {seedream_desc}");
        assert!(!flux_desc.contains("auto_2K"), "flux_desc must not advertise auto hint: {flux_desc}");
        assert!(flux_desc.contains("'16:9', '2:3', '1:1', '4:3', '3:4', '3:2'"), "flux_desc: {flux_desc}");
    }

    #[test]
    fn test_model_alias_deserialization() {
        let args = serde_json::json!({
            "prompt": "a cat",
            "aspect_ratio": "1:1",
            "model": "mai2pro",
        });
        let parsed: ImageGenArgs = serde_json::from_value(args).unwrap();
        assert_eq!(parsed.model.as_ref().map(|m| m.as_str()), Some("mai2pro"));
    }

    #[test]
    fn test_model_alias_absent_by_default() {
        let args = serde_json::json!({
            "prompt": "a cat",
            "aspect_ratio": "1:1",
        });
        let parsed: ImageGenArgs = serde_json::from_value(args).unwrap();
        assert!(parsed.model.is_none());
    }
}
