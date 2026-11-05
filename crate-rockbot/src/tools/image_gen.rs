use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::image_cache::{GeneratedImage, ImageCache};
use crate::provider::ImageProvider;
use crate::tool::Tool;
use crate::types::{
    ImageGenParams, ImageModelCatalog, ImageModelEntry, ImageModelTraits, ImageRoute,
    ImageSizeValue, ImageTier,
};
use crate::validated::{ModelAlias, NonEmptyString, ProviderName};

#[derive(Debug, Deserialize)]
struct ImageGenArgs {
    prompt: NonEmptyString,
    aspect_ratio: NonEmptyString,
    #[serde(default)]
    model: Option<ModelAlias>,
    /// Pin the provider route (issue #112); omitted ⇒ the alias default route.
    #[serde(default)]
    provider: Option<ProviderName>,
    /// Pick a route tier (`standard`/`pro`/`turbo`/`flash`).
    #[serde(default)]
    tier: Option<ImageTier>,
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

/// Per-room pro-tier accounting for the daily cap (issue #112). Day index is
/// UTC days since the Unix epoch; the map resets when the day rolls over.
struct ProTierUsage {
    day: u64,
    per_room: HashMap<String, u32>,
}

impl ProTierUsage {
    fn new() -> Self {
        Self {
            day: utc_day_index(),
            per_room: HashMap::new(),
        }
    }

    fn roll_over(&mut self) {
        let today = utc_day_index();
        if self.day != today {
            self.day = today;
            self.per_room.clear();
        }
    }

    /// Charge one pro-tier attempt to the room. `Err(used)` when the cap is
    /// already reached (the attempt is not counted).
    fn try_charge(&mut self, room: &str, cap: u32) -> std::result::Result<u32, u32> {
        self.roll_over();
        let used = self.per_room.get(room).copied().unwrap_or(0);
        if cap != 0 && used >= cap {
            return Err(used);
        }
        let used = used + 1;
        self.per_room.insert(room.to_string(), used);
        Ok(used)
    }
}

fn utc_day_index() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
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
    pro_daily_cap: u32,
    pro_usage: Mutex<ProTierUsage>,
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
        pro_daily_cap: u32,
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
            pro_daily_cap,
            pro_usage: Mutex::new(ProTierUsage::new()),
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
/// (issue #95). The manifest is the single source of truth — alias names,
/// labels, route providers/tiers, and deprecated markers all come from
/// `ImageModelCatalog`, so the text can never drift from `models/image.toml`.
fn build_description(catalog: &ImageModelCatalog) -> String {
    let models = if catalog.is_empty() {
        "(no image models configured)".to_string()
    } else {
        catalog
            .entries()
            .iter()
            .map(|e| {
                let routes = e
                    .routes
                    .iter()
                    .map(|r| {
                        let mut route = format!("{}:{}", r.provider_name, r.generate_model_id);
                        if let Some(edit) = &r.edit_model_id {
                            route.push_str(&format!(" (edit:{edit})"));
                        }
                        if r.tier != ImageTier::Standard {
                            route.push_str(&format!(" [{}]", r.tier));
                        }
                        route
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                let deprecated = if e.deprecated { ", DEPRECATED" } else { "" };
                format!("{} ({}, {}{})", e.alias, e.label, routes, deprecated)
            })
            .collect::<Vec<_>>()
            .join("; ")
    };

    let deprecated: Vec<&str> = catalog
        .entries()
        .iter()
        .filter(|e| e.deprecated)
        .map(|e| e.alias.as_str())
        .collect();

    let mut desc = format!(
        "Generate or edit an image. Provide a prompt and required aspect_ratio. \
         Available image models: {models}. \
         Default: '{}'. \
         Tiers: pick tier='turbo' or tier='flash' for cheap drafts and iteration; \
         tier='pro' spends more for final output — do not use pro for sketches. \
         Editing passes the input images to the same alias — a route with a dedicated \
         edit endpoint is preferred when one exists. \
         On a provider error the tool fails over to the next route once and reports \
         which provider/model served the image. \
         Standard ratios: '16:9', '2:3', '1:1', '4:3', '3:4', '3:2'. \
         User attachments are auto-provided as image_urls for editing. \
         Returns {{\"ok\": true, \"image_key\": \"...\", \"provider\": \"...\", \"model\": \"...\"}} — \
         share result as `![desc]({{image_key}})`.",
        catalog.default_alias(),
    );
    if !deprecated.is_empty() {
        desc.push_str(&format!(
            " Deprecated aliases (still accepted; prefer the replacement): {}.",
            deprecated.join(", ")
        ));
    }
    if catalog.supports_auto_aspect() {
        desc.push_str(" Seedream models also accept 'auto_2K' or 'auto_1K' to auto-select dimensions.");
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
                        "Image model alias (from models/image.toml). Default: '{}'. Editing reuses the same alias. Available: {}",
                        self.model_catalog.default_alias(),
                        available
                    )
                },
                "provider": {
                    "type": "string",
                    "description": "Optional provider pin for the alias route (e.g. 'fal', 'openrouter'). Omit to use the alias default route and its failover order."
                },
                "tier": {
                    "type": "string",
                    "enum": ["standard", "pro", "turbo", "flash"],
                    "description": "Optional route tier: 'turbo'/'flash' for cheap drafts and iteration, 'pro' for final output. Omit to use the alias default tier."
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

        // Resolve the alias to its catalog entry (explicit alias, else the
        // manifest's default alias). Routes are selected after input images
        // are known because edit calls prefer edit-capable routes (#100/#112).
        let entry: Option<&ImageModelEntry> = match &args.model {
            Some(alias) => Some(self.model_catalog.entry(alias.as_str()).ok_or_else(|| {
                RockBotError::ToolCallParse(format!(
                    "image_gen: model alias '{}' not in image model catalog — valid aliases: {}",
                    alias.as_str(),
                    self.model_catalog.valid_alias_list()
                ))
            })?),
            None => self.model_catalog.entry(self.model_catalog.default_alias()),
        };

        let will_edit = args.reference_image_key.is_some()
            || args
                .image_urls
                .as_ref()
                .is_some_and(|urls| !urls.is_empty());

        // Candidate routes: provider pin / tier filter first, then drop routes
        // whose provider has no backend in this process (issue #112).
        let candidates: Vec<&ImageRoute> = match entry {
            Some(e) => {
                let provider_pin = args.provider.as_ref().map(|p| p.as_str());
                let routes = e.route_candidates(provider_pin, args.tier, will_edit);
                if routes.is_empty() {
                    return Err(RockBotError::ToolCallParse(format!(
                        "image_gen: alias '{}' has no route for provider={} tier={} — \
                         routed providers: [{}]; tiers: [{}]",
                        e.alias,
                        provider_pin.unwrap_or("any"),
                        args.tier.map(|t| t.as_str()).unwrap_or("any"),
                        e.providers().join(", "),
                        e.tiers()
                            .iter()
                            .map(|t| t.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    )));
                }
                let route_providers: Vec<&str> =
                    routes.iter().map(|r| r.provider_name.as_str()).collect();
                let available: Vec<&ImageRoute> = routes
                    .into_iter()
                    .filter(|r| self.backends.contains_key(&r.provider_name))
                    .collect();
                if available.is_empty() {
                    return Err(RockBotError::ToolCallParse(format!(
                        "image_gen: alias '{}' routes only to providers with no available backend \
                         (routes: [{}]; configured backends: [{}])",
                        e.alias,
                        route_providers.join(", "),
                        self.backends.keys().cloned().collect::<Vec<_>>().join(", ")
                    )));
                }
                available
            }
            None => Vec::new(),
        };

        params.image_size = Some(ImageSizeValue::Preset(args.aspect_ratio.as_str().to_string()));
        params.size_tier = Some(self.default_image_size_tier.clone());
        // No safety-checker config knob since #114: the manifest no longer has
        // any fal seedream/v5 route, the only family the param applies to.

        // Data URIs are uploaded with the primary route's backend: fal needs
        // CDN URLs, OpenRouter returns data URIs, and the primary route is the
        // one the call will almost always use.
        let primary_backend: &ImageBackend = match candidates.first() {
            Some(route) => self
                .backends
                .get(&route.provider_name)
                .expect("candidate routes are filtered to available backends"),
            None => self.backends.get(&self.default_backend).ok_or_else(|| {
                RockBotError::Provider(format!(
                    "image_gen: default image provider '{}' has no available backend",
                    self.default_backend
                ))
            })?,
        };

        let mut collected_urls: Vec<String> = Vec::new();

        if let Some(ref key) = args.reference_image_key {
            if let Some(cached) = self.image_cache.get(key) {
                let data_uri = cached.data_uri();
                match self.upload_data_uri(primary_backend.t2i.as_ref(), &data_uri).await {
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
                        if let Ok(uploaded_url) = self.upload_data_uri(primary_backend.t2i.as_ref(), uri).await {
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

        // Try the preferred route, then advance to the next one once on a
        // provider error (issue #112). Every attempt is logged; the serving
        // route is returned to the caller.
        let attempts = if candidates.is_empty() {
            1
        } else {
            candidates.len().min(2)
        };
        let mut last_error: Option<RockBotError> = None;
        let mut generated: Option<(Vec<u8>, String, String, ImageTier, ImageModelTraits)> = None;

        for attempt in 0..attempts {
            let route = candidates.get(attempt).copied();
            let (provider, route_model, tier) = match route {
                Some(route) => {
                    let backend = self
                        .backends
                        .get(&route.provider_name)
                        .expect("candidate routes are filtered to available backends");
                    let provider = backend.select(is_img2img);
                    (
                        provider,
                        route.model_id_for_mode(is_img2img).to_string(),
                        route.tier,
                    )
                }
                None => {
                    let backend = self
                        .backends
                        .get(&self.default_backend)
                        .expect("default backend is validated before route selection");
                    let provider = backend.select(is_img2img);
                    (provider, provider.model_id().to_string(), ImageTier::Standard)
                }
            };

            // Text-to-image-only models reject input references (issue #104):
            // fail at the parse boundary with a clear error instead of sending
            // a body the provider refuses.
            let traits = ImageModelTraits::for_model_id(&route_model);
            if is_img2img && traits.text_to_image_only {
                let err = RockBotError::ToolCallParse(format!(
                    "image_gen: model '{}' is text-to-image only and cannot edit images — \
                     omit image_urls/reference_image_key or choose another alias: {}",
                    route_model,
                    self.model_catalog.valid_alias_list()
                ));
                warn!("{err}");
                last_error = Some(err);
                continue;
            }

            // Pro-tier cost guardrail (issue #112): charge the room before the
            // call so a provider failure cannot be used to bypass the cap.
            if tier == ImageTier::Pro {
                let mut usage = self
                    .pro_usage
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match usage.try_charge(room_id, self.pro_daily_cap) {
                    Ok(used) => debug!(
                        "image_gen pro-tier charge: room={} used={}/{}",
                        room_id, used, self.pro_daily_cap
                    ),
                    Err(used) => {
                        let err = RockBotError::ToolCallParse(format!(
                            "image_gen: per-room daily cap of {} pro-tier generations reached \
                             for room '{}' (used {}) — retry tomorrow or choose a non-pro \
                             alias/tier",
                            self.pro_daily_cap, room_id, used
                        ));
                        warn!("{err}; skipping route {}:{}", provider.provider_name(), route_model);
                        last_error = Some(err);
                        continue;
                    }
                }
            }

            params.model_id = route.map(|_| route_model.clone());

            debug!(
                "image_gen params: provider={} model={} img2img={} num_images={} quality={:?} output_format={:?} image_size={:?} image_urls_count={} prompt_len={} room={}",
                provider.provider_name(),
                route_model,
                is_img2img,
                params.num_images.unwrap_or(1),
                params.quality,
                params.output_format,
                params.image_size,
                params.image_urls.as_ref().map(|u| u.len()).unwrap_or(0),
                prompt.len(),
                room_id,
            );

            match provider.generate_image(&params).await {
                Ok(bytes) => {
                    info!(
                        "Image generated ({} / {}, tier={}): {} bytes elapsed_ms={}",
                        provider.provider_name(),
                        route_model,
                        tier,
                        bytes.len(),
                        t_start.elapsed().as_millis(),
                    );
                    generated = Some((
                        bytes,
                        provider.provider_name().to_string(),
                        route_model,
                        tier,
                        traits,
                    ));
                    break;
                }
                Err(e) => {
                    let next = candidates.get(attempt + 1);
                    match next {
                        Some(next) => warn!(
                            "image_gen: route {}:{} failed ({e}) — failing over to {}:{}",
                            provider.provider_name(),
                            route_model,
                            next.provider_name,
                            next.model_id_for_mode(is_img2img),
                        ),
                        None => warn!(
                            "image_gen: route {}:{} failed ({e}) — no failover route left",
                            provider.provider_name(),
                            route_model,
                        ),
                    }
                    last_error = Some(e);
                }
            }
        }

        let (image_bytes, served_provider, served_model, served_tier, traits) =
            generated.ok_or_else(|| {
                last_error.unwrap_or_else(|| {
                    RockBotError::Provider("image_gen: no route was attempted".into())
                })
            })?;

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

        info!(
            "image_gen ledger room={} provider={} model={} tier={} img2img={} elapsed_ms={}",
            room_id,
            served_provider,
            served_model,
            served_tier,
            is_img2img,
            t_start.elapsed().as_millis(),
        );

        let mut result = serde_json::json!({
            "ok": true,
            "webdav_path": webdav_path,
            "image_key": image_key,
            "provider": served_provider,
            "model": served_model,
            "tier": served_tier.as_str(),
        });
        if let Some(ref url) = share_url {
            result["share_url"] = serde_json::json!(url);
        }
        if traits.self_sized {
            result["note"] = serde_json::json!(
                "Model chose the output dimensions; aspect_ratio/size tier were omitted."
            );
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
            label: alias.to_string(),
            family: "test".to_string(),
            version: "0".to_string(),
            deprecated: false,
            routes: vec![ImageRoute {
                provider_name: provider.to_string(),
                generate_model_id: model_id.to_string(),
                edit_model_id: edit.map(|s| s.to_string()),
                tier: ImageTier::Standard,
                is_default: true,
            }],
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
           20,
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
        provider_name: &'static str,
        model_id: &'static str,
    }

    impl MockImageProvider {
        fn new() -> Self {
            Self {
                generate_result: std::sync::Mutex::new(Some(Ok(vec![1, 2, 3]))),
                upload_result: std::sync::Mutex::new(Some(Ok("https://cdn.example.com/uploaded.png".into()))),
                params_sink: std::sync::Arc::new(std::sync::Mutex::new(None)),
                provider_name: "mock",
                model_id: "mock-model",
            }
        }

        fn with_generate_error(e: RockBotError) -> Self {
            Self {
                generate_result: std::sync::Mutex::new(Some(Err(e))),
                upload_result: std::sync::Mutex::new(Some(Ok("https://cdn.example.com/uploaded.png".into()))),
                params_sink: std::sync::Arc::new(std::sync::Mutex::new(None)),
                provider_name: "mock",
                model_id: "mock-model",
            }
        }

        /// Named provider instance for multi-backend routing tests.
        fn named(provider_name: &'static str, model_id: &'static str) -> Self {
            Self {
                provider_name,
                model_id,
                ..Self::new()
            }
        }

        /// Named provider that always fails — failover tests.
        fn named_failing(provider_name: &'static str, model_id: &'static str) -> Self {
            Self {
                provider_name,
                model_id,
                ..Self::with_generate_error(RockBotError::Provider("provider down".into()))
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
            self.provider_name
        }

        fn model_id(&self) -> &str {
            self.model_id
        }
    }

    /// Provider that records every `ImageGenParams` it receives, then errors —
    /// used to assert what `execute()` sends downstream.
    fn make_recording_provider() -> (Box<dyn ImageProvider>, std::sync::Arc<std::sync::Mutex<Option<ImageGenParams>>>) {
        let sink = std::sync::Arc::new(std::sync::Mutex::new(None));
        let provider = MockImageProvider {
            params_sink: sink.clone(),
            ..MockImageProvider::with_generate_error(RockBotError::Provider(
                "capture-only".into(),
            ))
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
           20,
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
                    "gptimage",
                    "openai/gpt-image-2.6/flare/text-to-image",
                    Some("openai/gpt-image-2.6/flare/edit"),
                    "fal",
                ),
                entry(
                    "seedream5flash",
                    "bytedance-seed/seedream-5-0-flash",
                    None,
                    "openrouter",
                ),
            ],
            "seedream5flash",
        )
    }

    #[tokio::test]
    async fn test_execute_model_alias_override_reaches_params() {
        let (provider, sink) = make_recording_provider();
        let tool = make_tool(provider, make_multi_model_catalog());
        let args = serde_json::json!({
            "prompt": "a cat",
            "aspect_ratio": "1:1",
            "model": "gptimage",
        });
        let _ = tool.execute(&args.to_string()).await;
        let guard = sink.lock().unwrap();
        let captured = guard.as_ref().expect("provider received params");
        assert_eq!(
            captured.model_id.as_deref(),
            Some("openai/gpt-image-2.6/flare/text-to-image"),
            "model alias must resolve to the catalog's model id"
        );
    }

    #[tokio::test]
    async fn test_execute_model_alias_omitted_uses_default_alias_route() {
        let (provider, sink) = make_recording_provider();
        let catalog = ImageModelCatalog::new(
            vec![
                entry(
                    "gptimage",
                    "openai/gpt-image-2.6/flare/text-to-image",
                    Some("openai/gpt-image-2.6/flare/edit"),
                    "fal",
                ),
                entry(
                    "seedream5flash",
                    "bytedance-seed/seedream-5-0-flash",
                    None,
                    "openrouter",
                ),
            ],
            "gptimage",
        );
        let tool = make_tool(provider, catalog);
        let args = serde_json::json!({
            "prompt": "a cat",
            "aspect_ratio": "1:1",
        });
        let _ = tool.execute(&args.to_string()).await;
        let guard = sink.lock().unwrap();
        let captured = guard.as_ref().expect("provider received params");
        assert_eq!(
            captured.model_id.as_deref(),
            Some("openai/gpt-image-2.6/flare/text-to-image"),
            "omitted alias adopts the default alias's preferred route (#112)"
        );
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
        assert!(
            msg.contains("gptimage") && msg.contains("seedream5flash"),
            "err should list valid aliases: {msg}"
        );
    }

    #[tokio::test]
    async fn test_execute_dropped_alias_is_rejected_with_valid_alias_list() {
        // Issue #114 acceptance: a room still pinned to a removed alias gets a
        // clear parse-boundary error naming the surviving roster.
        let tool = make_tool(
            Box::new(MockImageProvider::with_generate_error(RockBotError::Provider("x".into()))),
            make_multi_model_catalog(),
        );
        let err = tool
            .execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"mai2pro"}"#)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not in image model catalog"), "err: {msg}");
        assert!(msg.contains("alias 'mai2pro'"), "err should name the dropped alias: {msg}");
        assert!(
            msg.contains("valid aliases: gptimage, seedream5flash"),
            "err should list the surviving roster: {msg}"
        );
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
           20,
            make_webdav(),
            make_image_cache(),
        );

        // gptimage → fal backend
        let _ = tool.execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"gptimage"}"#).await;
        assert_eq!(
            fal_sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("openai/gpt-image-2.6/flare/text-to-image"),
            "fal-tagged alias must hit the fal backend"
        );
        assert!(or_sink.lock().unwrap().is_none(), "fal alias must not hit openrouter");

        // seedream5flash → openrouter backend
        let _ = tool.execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"seedream5flash"}"#).await;
        assert_eq!(
            or_sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("bytedance-seed/seedream-5-0-flash"),
            "openrouter-tagged alias must hit the openrouter backend"
        );

        // omitted model → default alias 'seedream5flash' (openrouter), so its
        // entry resolves mode-aware and sets the t2i id explicitly
        let _ = tool.execute(r#"{"prompt":"a cat","aspect_ratio":"1:1"}"#).await;
        let or_captured = or_sink.lock().unwrap();
        let captured = or_captured.as_ref().unwrap();
        assert_eq!(
            captured.model_id.as_deref(),
            Some("bytedance-seed/seedream-5-0-flash"),
            "omitted alias adopts the default alias entry (mode-aware)"
        );
        assert_eq!(captured.prompt.as_str(), "a cat");
    }

    #[tokio::test]
    async fn test_execute_edit_mode_swaps_to_dedicated_endpoint() {
        // Default alias = gptimage (fal pair); tool default backend = fal.
        let catalog = ImageModelCatalog::new(
            vec![
                entry(
                    "gptimage",
                    "openai/gpt-image-2.6/flare/text-to-image",
                    Some("openai/gpt-image-2.6/flare/edit"),
                    "fal",
                ),
                entry(
                    "seedream5flash",
                    "bytedance-seed/seedream-5-0-flash",
                    None,
                    "openrouter",
                ),
            ],
            "gptimage",
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
           20,
            make_webdav(),
            make_image_cache(),
        );

        // Edit call WITHOUT explicit model → default alias pair swaps to the
        // dedicated edit endpoint id (issue #100)
        let _ = tool.execute(r#"{"prompt":"recolor","aspect_ratio":"1:1","image_urls":["https://example.com/in.png"]}"#).await;
        assert_eq!(
            fal_sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("openai/gpt-image-2.6/flare/edit"),
            "default paired alias must swap to its edit endpoint in edit mode"
        );

        // Same-model editing: seedream5flash (no companion) keeps the plain id in edit mode
        let _ = tool.execute(r#"{"prompt":"recolor","aspect_ratio":"1:1","model":"seedream5flash","image_urls":["https://example.com/in.png"]}"#).await;
        assert_eq!(
            or_sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("bytedance-seed/seedream-5-0-flash"),
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
            make_multi_model_catalog(), // seedream5flash → openrouter, but no openrouter backend
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
           20,
            make_webdav(),
            make_image_cache(),
        );
        let err = tool
            .execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"seedream5flash"}"#)
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
        assert_eq!(model["enum"], serde_json::json!(["gptimage", "seedream5flash"]));
    }

    #[test]
    fn test_schema_model_has_no_enum_when_catalog_empty() {
        let tool = make_tool(
            Box::new(MockImageProvider::new()),
            ImageModelCatalog::new(Vec::new(), "gptimage"),
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
        assert!(
            desc.contains(
                "seedream5flash (seedream5flash, openrouter:bytedance-seed/seedream-5-0-flash)"
            ),
            "desc: {desc}"
        );
        assert!(
            desc.contains(
                "gptimage (gptimage, fal:openai/gpt-image-2.6/flare/text-to-image \
                 (edit:openai/gpt-image-2.6/flare/edit))"
            ),
            "desc: {desc}"
        );
        assert!(desc.contains("Default: 'seedream5flash'"), "desc: {desc}");
        assert!(
            !desc.contains("auto_2K") && !desc.contains("auto_1K"),
            "surviving roster has no fal seedream/v5 route → no auto hint: {desc}"
        );
    }

    #[test]
    fn test_tool_description_omits_auto_hint_without_seedream() {
        let tool = make_tool(Box::new(MockImageProvider::new()), make_model_catalog());
        let desc = tool.description();
        assert!(desc.contains("flux (flux, fal:fal-ai/flux/schnell)"), "desc: {desc}");
        assert!(!desc.contains("auto_2K") && !desc.contains("auto_1K"), "no seedream → no auto hint: {desc}");
        assert!(desc.contains("Default: 'flux'"), "desc: {desc}");
    }

    #[test]
    fn test_tool_description_empty_catalog() {
        let tool = make_tool(
            Box::new(MockImageProvider::new()),
            ImageModelCatalog::new(Vec::new(), "gptimage"),
        );
        let desc = tool.description();
        assert!(desc.contains("no image models configured"), "desc: {desc}");
        assert!(!desc.contains("auto_2K"), "no seedream → no auto hint: {desc}");
    }

    #[test]
    fn test_aspect_ratio_description_auto_hint_conditional() {
        // The predicate is model-id driven, not alias driven: a catalog with a
        // fal seedream/v5 slug still advertises the auto-dimensional hint even
        // though the current roster has no such route.
        let seedream_catalog = ImageModelCatalog::new(
            vec![entry(
                "synthetic-seedream",
                "bytedance/seedream/v5/pro/text-to-image",
                None,
                "fal",
            )],
            "synthetic-seedream",
        );
        let seedream_tool = make_tool(Box::new(MockImageProvider::new()), seedream_catalog);
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
            "model": "qwenimage",
        });
        let parsed: ImageGenArgs = serde_json::from_value(args).unwrap();
        assert_eq!(parsed.model.as_ref().map(|m| m.as_str()), Some("qwenimage"));
    }

    #[test]
    fn test_model_alias_absent_by_default() {
        let args = serde_json::json!({
            "prompt": "a cat",
            "aspect_ratio": "1:1",
        });
        let parsed: ImageGenArgs = serde_json::from_value(args).unwrap();
        assert!(parsed.model.is_none());
        assert!(parsed.provider.is_none());
        assert!(parsed.tier.is_none());
    }

    // ----- Provider routes: pin, tier, failover, cost cap (issue #112) -----

    fn route_entry(
        alias: &str,
        routes: Vec<(&str, &str, Option<&str>, ImageTier, bool)>,
    ) -> ImageModelEntry {
        ImageModelEntry {
            alias: alias.to_string(),
            label: alias.to_string(),
            family: "test".to_string(),
            version: "0".to_string(),
            deprecated: false,
            routes: routes
                .into_iter()
                .map(|(provider, generate, edit, tier, is_default)| ImageRoute {
                    provider_name: provider.to_string(),
                    generate_model_id: generate.to_string(),
                    edit_model_id: edit.map(|s| s.to_string()),
                    tier,
                    is_default,
                })
                .collect(),
        }
    }

    /// Synthetic two-provider alias used only to exercise generic route
    /// ordering (pin + failover). The real roster has no cross-provider
    /// routes (#114).
    fn route_demo_entry() -> ImageModelEntry {
        route_entry(
            "route-demo",
            vec![
                (
                    "openrouter",
                    "demo/generate",
                    Some("demo/generate"),
                    ImageTier::Standard,
                    true,
                ),
                (
                    "fal",
                    "demo/generate-fal",
                    Some("demo/edit-fal"),
                    ImageTier::Standard,
                    false,
                ),
            ],
        )
    }

    fn make_multi_backend_tool(
        catalog: ImageModelCatalog,
        fal: Box<dyn ImageProvider>,
        or: Box<dyn ImageProvider>,
        pro_daily_cap: u32,
    ) -> ImageGenTool {
        ImageGenTool::new(
            HashMap::from([
                ("fal".to_string(), ImageBackend::new(fal, None)),
                ("openrouter".to_string(), ImageBackend::new(or, None)),
            ]),
            "openrouter".to_string(),
            catalog,
            "medium".into(),
            "png".into(),
            1,
            "4K".into(),
            pro_daily_cap,
            make_webdav(),
            make_image_cache(),
        )
    }

    #[tokio::test]
    async fn test_execute_provider_arg_pins_route() {
        let (fal, fal_sink) = make_recording_provider();
        let (or, or_sink) = make_recording_provider();
        let tool = make_multi_backend_tool(
            ImageModelCatalog::new(vec![route_demo_entry()], "route-demo"),
            fal,
            or,
            0,
        );

        let _ = tool
            .execute(
                r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"route-demo","provider":"fal"}"#,
            )
            .await;

        assert_eq!(
            fal_sink
                .lock()
                .unwrap()
                .as_ref()
                .expect("pinned fal backend received the call")
                .model_id
                .as_deref(),
            Some("demo/generate-fal")
        );
        assert!(
            or_sink.lock().unwrap().is_none(),
            "provider pin must not call the openrouter backend"
        );
    }

    #[tokio::test]
    async fn test_execute_unknown_provider_pin_is_parse_error() {
        let tool = make_multi_backend_tool(
            ImageModelCatalog::new(vec![route_demo_entry()], "route-demo"),
            Box::new(MockImageProvider::named("fal", "fal-default")),
            Box::new(MockImageProvider::named("openrouter", "or-default")),
            0,
        );
        let err = tool
            .execute(
                r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"route-demo","provider":"replicate"}"#,
            )
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no route for provider=replicate"), "err: {msg}");
        assert!(msg.contains("openrouter, fal"), "err should list routed providers: {msg}");
    }

    #[tokio::test]
    async fn test_execute_tier_selects_route_over_default() {
        // Generic tier-selection mechanism; the surviving roster exposes
        // pro (qwen3), turbo (qwenimage) and flash (seedream5flash) tiers
        // (#114, #118).
        let entry = route_entry(
            "tier-demo",
            vec![
                (
                    "openrouter",
                    "demo/pro",
                    Some("demo/pro"),
                    ImageTier::Pro,
                    true,
                ),
                (
                    "openrouter",
                    "demo/flash",
                    Some("demo/flash"),
                    ImageTier::Flash,
                    false,
                ),
            ],
        );
        let (provider, sink) = make_recording_provider();
        let tool = make_multi_backend_tool(
            ImageModelCatalog::new(vec![entry], "tier-demo"),
            Box::new(MockImageProvider::named("fal", "fal-default")),
            provider,
            0,
        );

        let _ = tool
            .execute(
                r#"{"prompt":"a sketch","aspect_ratio":"1:1","model":"tier-demo","tier":"flash"}"#,
            )
            .await;

        assert_eq!(
            sink.lock().unwrap().as_ref().unwrap().model_id.as_deref(),
            Some("demo/flash"),
            "tier=flash must pick the flash route, not the pro default"
        );
    }

    #[tokio::test]
    async fn test_execute_unknown_tier_is_parse_error() {
        let tool = make_multi_backend_tool(
            ImageModelCatalog::new(vec![route_demo_entry()], "route-demo"),
            Box::new(MockImageProvider::named("fal", "fal-default")),
            Box::new(MockImageProvider::named("openrouter", "or-default")),
            0,
        );
        let err = tool
            .execute(
                r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"route-demo","tier":"flash"}"#,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no route for provider=any tier=flash"), "err: {err}");
    }

    #[tokio::test]
    async fn test_execute_fails_over_to_next_route() {
        let (fal, fal_sink) = make_recording_provider();
        let tool = make_multi_backend_tool(
            ImageModelCatalog::new(vec![route_demo_entry()], "route-demo"),
            fal,
            Box::new(MockImageProvider::named_failing("openrouter", "or-default")),
            0,
        );

        // The first (openrouter) route fails; the fal fallback is attempted and
        // reaches the provider. WebDAV upload then fails in this unit harness,
        // so the overall call still errors — the failover is proven by the
        // second backend receiving the request.
        let _ = tool
            .execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"route-demo"}"#)
            .await;

        assert_eq!(
            fal_sink
                .lock()
                .unwrap()
                .as_ref()
                .expect("failover route received the call")
                .model_id
                .as_deref(),
            Some("demo/generate-fal")
        );
    }

    #[tokio::test]
    async fn test_execute_pro_tier_daily_cap() {
        let entry = route_entry(
            "pro-demo",
            vec![(
                "openrouter",
                "demo/pro",
                Some("demo/pro"),
                ImageTier::Pro,
                true,
            )],
        );
        let tool = make_multi_backend_tool(
            ImageModelCatalog::new(vec![entry], "pro-demo"),
            Box::new(MockImageProvider::named("fal", "fal-default")),
            Box::new(MockImageProvider::named("openrouter", "or-default")),
            1,
        );

        // First call is charged (and then fails at the WebDAV upload).
        let _ = tool
            .execute(r#"{"prompt":"final art","aspect_ratio":"1:1","model":"pro-demo","room_id":"room-a"}"#)
            .await;

        let err = tool
            .execute(r#"{"prompt":"final art again","aspect_ratio":"1:1","model":"pro-demo","room_id":"room-a"}"#)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("daily cap of 1 pro-tier generations"),
            "err: {err}"
        );

        // A different room has its own budget.
        let other = tool
            .execute(r#"{"prompt":"final art","aspect_ratio":"1:1","model":"pro-demo","room_id":"room-b"}"#)
            .await;
        assert!(
            !other
                .as_ref()
                .err()
                .map(|e| e.to_string().contains("daily cap"))
                .unwrap_or(false),
            "cap is per-room: {other:?}"
        );
    }
}
