use serde::{Deserialize, Serialize};

use crate::error::{Result, RockBotError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    #[serde(rename = "tool_calls")]
    ToolUse,
    #[serde(rename = "content_filter")]
    ContentFilter,
    #[serde(rename = "insufficient_system_resource")]
    InsufficientSystemResource,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Multipart(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    #[serde(rename = "image_url")]
    ImageUrl {
        image_url: ImageUrlPayload,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageUrlPayload {
    pub url: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user_with_image(text: impl Into<String>, image_data_uri: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Multipart(vec![
                ContentPart::Text {
                    text: text.into(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrlPayload {
                        url: image_data_uri.into(),
                        detail: Some("high".into()),
                    },
                },
            ]),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user_with_image_url(
        text: impl Into<String>,
        image_url: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Multipart(vec![
                ContentPart::Text {
                    text: text.into(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrlPayload {
                        url: image_url.into(),
                        detail: Some("high".into()),
                    },
                },
            ]),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn user_with_images(
        text: impl Into<String>,
        image_data_uris: Vec<String>,
    ) -> Self {
        let mut parts: Vec<ContentPart> = Vec::with_capacity(image_data_uris.len() + 1);
        parts.push(ContentPart::Text {
            text: text.into(),
        });
        for uri in image_data_uris {
            parts.push(ContentPart::ImageUrl {
                image_url: ImageUrlPayload {
                    url: uri,
                    detail: Some("high".into()),
                },
            });
        }
        Self {
            role: Role::User,
            content: MessageContent::Multipart(parts),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
        reasoning_content: Option<String>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            reasoning_content,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            reasoning_content: None,
        }
    }

    pub fn text_content(&self) -> Option<&str> {
        match &self.content {
            MessageContent::Text(t) => Some(t.as_str()),
            MessageContent::Multipart(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub call_type: String,
    pub function: FunctionCall,
}

fn default_tool_type() -> String {
    "function".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: arguments.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDef {
    #[serde(rename = "type", default = "default_tool_type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub strict: Option<bool>,
}

impl ToolDef {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".into(),
            function: FunctionDef {
                name: name.into(),
                description: Some(description.into()),
                parameters: Some(parameters),
                strict: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "is_false", default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub thinking_type: String,
}

impl ThinkingConfig {
    pub fn enabled() -> Self {
        Self {
            thinking_type: "enabled".into(),
        }
    }

    pub fn disabled() -> Self {
        Self {
            thinking_type: "disabled".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionResult {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub finish: FinishReason,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone)]
pub enum ImageSizeValue {
    Preset(String),
    Custom { width: u32, height: u32 },
}

#[derive(Debug, Clone)]
pub struct ImageGenParams {
    pub prompt: String,
    pub quality: Option<String>,
    pub image_size: Option<ImageSizeValue>,
    pub size_tier: Option<String>,
    pub output_format: Option<String>,
    pub num_images: Option<u32>,
    pub model_id: Option<String>,
    pub image_urls: Option<Vec<String>>,
    pub enable_safety_checker: Option<bool>,
}

impl ImageGenParams {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            quality: None,
            image_size: None,
            size_tier: None,
            output_format: None,
            num_images: None,
            model_id: None,
            image_urls: None,
            enable_safety_checker: None,
        }
    }

    pub fn resolve_image_size(&self) -> Option<serde_json::Value> {
        match &self.image_size {
            Some(ImageSizeValue::Preset(name)) => Self::lookup_preset(name)
                .map(|(w, h)| serde_json::json!({ "width": w, "height": h }))
                .or_else(|| Some(serde_json::json!(name))),
            Some(ImageSizeValue::Custom { width, height }) => {
                Some(serde_json::json!({ "width": width, "height": height }))
            }
            None => None,
        }
    }

    fn lookup_preset(name: &str) -> Option<(u32, u32)> {
        match name {
            "square_hd" | "1:1" => Some((2880, 2880)),
            "landscape_16_9" | "16:9" => Some((3840, 2160)),
            "portrait_16_9" | "9:16" => Some((2160, 3840)),
            "landscape_4_3" | "4:3" => Some((3312, 2480)),
            "portrait_4_3" | "3:4" => Some((2480, 3312)),
            "landscape_3_2" | "3:2" => Some((3504, 2336)),
            "portrait_2_3" | "2:3" => Some((2336, 3504)),
            "square" => Some((512, 512)),
            _ => None,
        }
    }

    pub fn validate_dimensions(&self) -> Result<()> {
        if let Some(ImageSizeValue::Custom { width, height }) = &self.image_size {
            let pixels = (*width as u64) * (*height as u64);
            let max_edge = std::cmp::max(*width, *height);
            let min_edge = std::cmp::min(*width, *height);

            if max_edge > 3840 {
                return Err(RockBotError::Provider(format!(
                    "image_size max edge must be ≤3840px (got {}×{}).",
                    width, height
                )));
            }

            if min_edge > 0 && max_edge as f64 / min_edge as f64 > 3.0 {
                return Err(RockBotError::Provider(format!(
                    "image_size aspect ratio must be ≤3:1 (got {}×{}).",
                    width, height
                )));
            }

            if !(655_360..=8_294_400).contains(&pixels) {
                return Err(RockBotError::Provider(format!(
                    "image_size pixel count must be 655,360–8,294,400 (got {} from {}×{}).",
                    pixels, width, height
                )));
            }

            if *width % 16 != 0 || *height % 16 != 0 {
                return Err(RockBotError::Provider(format!(
                    "image_size dimensions must be multiples of 16 (got {}×{}).",
                    width, height
                )));
            }
        }
        Ok(())
    }
}

/// Config-derived catalog of available image model aliases → model ids,
/// each tagged with the `[[image_providers]]` entry name that owns it.
///
/// Canonical shared type (see `_dfd/tools/image-gen/structures.md` §3):
/// produced by `main.rs` from the `models` maps of **every** configured image
/// provider entry backed by a supported kind (fal / openrouter), plus the
/// `[image_model]` default alias, and consumed by `ImageGenTool` for
/// per-call model selection and provider routing (`resolve` returns the
/// provider name of the alias). Construction is infallible — config leniency
/// is preserved (an unresolvable default alias is kept as-is and never blocks
/// tool registration); the tool rejects unknown LLM-provided aliases at the
/// parse boundary instead.
///
/// One alias per model family (issue #100): `edit_model_id` is `Some` only
/// where the provider genuinely uses a separate edit endpoint (fal); editing
/// otherwise reuses the t2i id, so no `*_edit` selectable aliases exist.
/// Chat-only models never enter this catalog — image-provider default tables
/// are role-scoped from chat defaults at config load (issue #99).
/// Model-id marker for the seedream v5 family — the only Fal models whose
/// request bodies accept the `auto_1K`/`auto_2K` auto-dimensional aspect
/// strings. Shared between the catalog's `supports_auto_aspect()` predicate
/// and `FalAiProvider`'s request builder so the tool description hint and the
/// wire body can never disagree.
pub const SEEDREAM_V5_MARKER: &str = "seedream/v5";

/// Model-id marker for Ming Image 0.1 Design (issue #104) — the only model
/// known to be both self-sized (rejects explicit sizes/aspect ratios) and
/// text-to-image only (accepts no input references). Single source for both
/// `ImageModelTraits` flags so tool behaviour and the OpenRouter request
/// builder can never disagree about this model.
const MING_DESIGN_MODEL_ID: &str = "inclusionai/ming-image-0.1-design";

/// Wire-level per-model traits that the OpenRouter image catalog does **not**
/// advertise (issue #104). Derived from the model id once at the parse
/// boundary — canonical shared type consumed by `ImageGenTool` (edit-call
/// rejection + result hint) and `OpenRouterImageProvider` (size-param bypass).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageModelTraits {
    /// Model picks its own output dimensions: the provider rejects
    /// `resolution` / `aspect_ratio` params, so the request builder omits them.
    pub self_sized: bool,
    /// Model accepts no input references: edit calls must fail with a clear
    /// text-to-image-only error instead of reaching the provider.
    pub text_to_image_only: bool,
}

impl ImageModelTraits {
    /// Derive the traits for a model id. Unknown ids get all-default (`false`).
    pub fn for_model_id(model_id: &str) -> Self {
        let is_ming_design = model_id == MING_DESIGN_MODEL_ID;
        Self {
            self_sized: is_ming_design,
            text_to_image_only: is_ming_design,
        }
    }
}

/// Route tier of an image model (issue #112): `standard` when unspecified,
/// otherwise a quality/speed split the LLM can request explicitly. `Pro` is
/// the only tier that counts against the per-room pro-tier daily cap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageTier {
    #[default]
    Standard,
    Pro,
    Turbo,
    Flash,
}

impl ImageTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Pro => "pro",
            Self::Turbo => "turbo",
            Self::Flash => "flash",
        }
    }
}

impl std::fmt::Display for ImageTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ImageTier {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "standard" => Ok(Self::Standard),
            "pro" => Ok(Self::Pro),
            "turbo" => Ok(Self::Turbo),
            "flash" => Ok(Self::Flash),
            other => Err(format!(
                "unknown image model tier '{other}' — valid tiers: standard, pro, turbo, flash"
            )),
        }
    }
}

/// One provider route of an alias (issue #112): the slug pair a provider
/// serves for the same model family. A route with `edit_model_id: None` is
/// generate-only — edit calls fall through to the next route (or reuse the
/// generate id when no route declares an edit endpoint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRoute {
    /// Owning `[[image_providers]]` entry name — selects the backend.
    pub provider_name: String,
    /// Text-to-image slug sent to the provider.
    pub generate_model_id: String,
    /// Dedicated edit-endpoint slug; `None` ⇒ generate-only route.
    pub edit_model_id: Option<String>,
    /// Quality/speed tier this route serves.
    pub tier: ImageTier,
    /// Preferred route of the alias (first if none is flagged).
    pub is_default: bool,
}

impl ImageRoute {
    /// Mode-appropriate model id for this route.
    pub fn model_id_for_mode(&self, is_edit: bool) -> &str {
        match (is_edit, &self.edit_model_id) {
            (true, Some(edit_id)) => edit_id,
            _ => &self.generate_model_id,
        }
    }
}

/// Wire-level traits of an image model, plus the fal request-shape selector.
///
/// Some fal endpoints do not accept the `{"width","height"}` `image_size`
/// object the provider sends by default: `bria/fibo-2/*` takes an
/// `aspect_ratio` string, and `google/nano-banana-2.1*` takes `aspect_ratio`
/// plus a `resolution` tier (verified against the fal OpenAPI schemas,
/// 2026-11-05). Deriving the shape from the wire model id keeps the request
/// builder and the manifest in agreement without per-alias code.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FalSizeParam {
    /// `image_size: {"width", "height"}` — gpt-image, seedream, moss.
    #[default]
    ImageSizeObject,
    /// `aspect_ratio: "16:9"` — bria/fibo-2.
    AspectRatio,
    /// `aspect_ratio` + `resolution: "1K"|"2K"|"4K"` — nano-banana 2.1.
    AspectRatioWithResolution,
}

impl FalSizeParam {
    pub fn for_model_id(model_id: &str) -> Self {
        if model_id.starts_with("google/nano-banana-2.1") {
            Self::AspectRatioWithResolution
        } else if model_id.starts_with("bria/fibo-2/") {
            Self::AspectRatio
        } else {
            Self::ImageSizeObject
        }
    }
}

/// One catalog entry: a selectable alias and its ordered provider routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageModelEntry {
    pub alias: String,
    /// Human-facing name shown in the tool description.
    pub label: String,
    /// Model family (`openai`, `google`, `qwen`, …).
    pub family: String,
    /// Family version string (`2.6`, `2.1`, …).
    pub version: String,
    /// Deprecated aliases stay selectable for one release cycle.
    pub deprecated: bool,
    /// Ordered routes (preferred first). Never empty after validation.
    pub routes: Vec<ImageRoute>,
}

impl ImageModelEntry {
    /// Preferred route: the one flagged `is_default`, else the first.
    ///
    /// Entries reaching the catalog are validated to carry at least one route
    /// (`image_manifest` rejects zero-route models), so the expect only fires
    /// on a broken internal invariant.
    pub fn default_route(&self) -> &ImageRoute {
        self.routes
            .iter()
            .find(|r| r.is_default)
            .or_else(|| self.routes.first())
            .expect("ImageModelEntry invariant: at least one route")
    }

    /// Mode-appropriate model id of the preferred route.
    pub fn model_id_for_mode(&self, is_edit: bool) -> &str {
        self.default_route().model_id_for_mode(is_edit)
    }

    /// Routes eligible for a call, preferred first: optional provider pin,
    /// optional tier, then — for edit calls — routes with a dedicated edit
    /// endpoint before generate-only routes.
    pub fn route_candidates(
        &self,
        provider: Option<&str>,
        tier: Option<ImageTier>,
        is_edit: bool,
    ) -> Vec<&ImageRoute> {
        let mut routes: Vec<&ImageRoute> = self
            .routes
            .iter()
            .filter(|r| provider.is_none_or(|p| r.provider_name == p))
            .filter(|r| tier.is_none_or(|t| r.tier == t))
            .collect();
        routes.sort_by_key(|r| !r.is_default);
        if is_edit {
            // Stable sort keeps the default preference inside each group.
            routes.sort_by_key(|r| r.edit_model_id.is_none());
        }
        routes
    }

    /// Provider names this entry can route to, in declaration order.
    pub fn providers(&self) -> Vec<&str> {
        self.routes.iter().map(|r| r.provider_name.as_str()).collect()
    }

    /// Tiers this entry exposes, in declaration order (deduplicated).
    pub fn tiers(&self) -> Vec<ImageTier> {
        let mut tiers: Vec<ImageTier> = Vec::new();
        for r in &self.routes {
            if !tiers.contains(&r.tier) {
                tiers.push(r.tier);
            }
        }
        tiers
    }

    /// Wire-level traits of the preferred route's text-to-image id (#104).
    pub fn traits(&self) -> ImageModelTraits {
        ImageModelTraits::for_model_id(&self.default_route().generate_model_id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImageModelCatalog {
    entries: Vec<ImageModelEntry>,
    default_alias: String,
}

impl ImageModelCatalog {
    pub fn new(
        mut entries: Vec<ImageModelEntry>,
        default_alias: impl Into<String>,
    ) -> Self {
        entries.sort_by(|a, b| a.alias.cmp(&b.alias));
        Self {
            entries,
            default_alias: default_alias.into(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries in alias-sorted order — feeds the derived tool description.
    pub fn entries(&self) -> &[ImageModelEntry] {
        &self.entries
    }

    /// Resolve a config alias to its preferred route's `(model_id,
    /// provider_name)` pair — the tool routes the call to that provider's
    /// backend.
    pub fn resolve(&self, alias: &str) -> Option<(&str, &str)> {
        self.entry(alias).map(|e| {
            let route = e.default_route();
            (route.generate_model_id.as_str(), route.provider_name.as_str())
        })
    }

    /// Resolve to the full entry (all routes included).
    pub fn entry(&self, alias: &str) -> Option<&ImageModelEntry> {
        self.entries.iter().find(|e| e.alias == alias)
    }

    /// Mode-appropriate model id for an alias's preferred route.
    pub fn resolve_for_mode(&self, alias: &str, is_edit: bool) -> Option<(&str, &str)> {
        self.entry(alias).map(|e| {
            let route = e.default_route();
            (
                route.model_id_for_mode(is_edit),
                route.provider_name.as_str(),
            )
        })
    }

    /// Aliases in sorted order — feeds the tool schema `enum` (deprecated
    /// aliases included so old rooms keep working).
    pub fn allowed_aliases(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.alias.as_str()).collect()
    }

    /// Provider names actually referenced by at least one route (sorted).
    pub fn referenced_providers(&self) -> Vec<&str> {
        let mut providers: Vec<&str> = self
            .entries
            .iter()
            .flat_map(|e| e.routes.iter().map(|r| r.provider_name.as_str()))
            .collect();
        providers.sort_unstable();
        providers.dedup();
        providers
    }

    /// Drop routes whose provider has no backend, then entries left with no
    /// routes. Used at registration so the schema never advertises a provider
    /// this process cannot call.
    pub fn restricted_to_providers(&self, available: &std::collections::HashSet<&str>) -> Self {
        let entries = self
            .entries
            .iter()
            .filter_map(|e| {
                let routes: Vec<ImageRoute> = e
                    .routes
                    .iter()
                    .filter(|r| available.contains(r.provider_name.as_str()))
                    .cloned()
                    .collect();
                if routes.is_empty() {
                    None
                } else {
                    Some(ImageModelEntry {
                        routes,
                        ..e.clone()
                    })
                }
            })
            .collect();
        Self {
            entries,
            default_alias: self.default_alias.clone(),
        }
    }

    /// Move the default alias's first route owned by `provider` to the front
    /// (and flag it default) — used when no manifest supplies a default route.
    pub fn prefer_provider(&mut self, provider: &str) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.alias == self.default_alias)
            && let Some(idx) = entry
                .routes
                .iter()
                .position(|r| r.provider_name == provider)
        {
            let mut route = entry.routes.remove(idx);
            route.is_default = true;
            for other in &mut entry.routes {
                other.is_default = false;
            }
            entry.routes.insert(0, route);
        }
    }

    /// `true` iff any route's model id is in the seedream v5 family —
    /// determines whether the tool advertises the `auto_1K`/`auto_2K`
    /// aspect strings to the LLM.
    pub fn supports_auto_aspect(&self) -> bool {
        self.entries.iter().any(|e| {
            e.routes
                .iter()
                .any(|r| r.generate_model_id.contains(SEEDREAM_V5_MARKER))
        })
    }

    /// `"a, b, c"` or `"(none configured)"` — for parse-error messages.
    pub fn valid_alias_list(&self) -> String {
        if self.entries.is_empty() {
            "(none configured)".to_string()
        } else {
            self.allowed_aliases().join(", ")
        }
    }

    pub fn default_alias(&self) -> &str {
        &self.default_alias
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_dimensions_preset_passes() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Preset("16:9".into())),
            size_tier: None, output_format: None, num_images: None,
            model_id: None,             image_urls: None,
            enable_safety_checker: None,
        };
        assert!(p.validate_dimensions().is_ok(), "Preset should bypass pixel validation");
    }

    #[test]
    fn test_validate_dimensions_custom_valid() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Custom { width: 1920, height: 1088 }),
            size_tier: None, output_format: None, num_images: None,
            model_id: None,             image_urls: None,
            enable_safety_checker: None,
        };
        assert!(p.validate_dimensions().is_ok());
    }

    #[test]
    fn test_validate_dimensions_none_passes() {
        let p = ImageGenParams::new("t");
        assert!(p.validate_dimensions().is_ok());
    }

    #[test]
    fn test_validate_dimensions_max_edge_exceeded() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Custom { width: 4000, height: 2000 }),
            size_tier: None, output_format: None, num_images: None,
            model_id: None,             image_urls: None,
            enable_safety_checker: None,
        };
        let err = p.validate_dimensions().unwrap_err();
        assert!(err.to_string().contains("max edge"));
        assert!(err.to_string().contains("3840"));
    }

    #[test]
    fn test_validate_dimensions_aspect_ratio_exceeded() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Custom { width: 3500, height: 1000 }),
            size_tier: None, output_format: None, num_images: None,
            model_id: None,             image_urls: None,
            enable_safety_checker: None,
        };
        let err = p.validate_dimensions().unwrap_err();
        assert!(err.to_string().contains("aspect ratio"));
    }

    #[test]
    fn test_validate_dimensions_pixel_count_too_low() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Custom { width: 800, height: 800 }),
            size_tier: None, output_format: None, num_images: None,
            model_id: None,             image_urls: None,
            enable_safety_checker: None,
        };
        let err = p.validate_dimensions().unwrap_err();
        assert!(err.to_string().contains("pixel count"));
    }

    #[test]
    fn test_validate_dimensions_pixel_count_too_high() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Custom { width: 3840, height: 3840 }),
            size_tier: None, output_format: None, num_images: None,
            model_id: None,             image_urls: None,
            enable_safety_checker: None,
        };
        let err = p.validate_dimensions().unwrap_err();
        assert!(err.to_string().contains("pixel count"));
    }

    #[test]
    fn test_validate_dimensions_not_multiple_of_16() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Custom { width: 1920, height: 1081 }),
            size_tier: None, output_format: None, num_images: None,
            model_id: None,             image_urls: None,
            enable_safety_checker: None,
        };
        let err = p.validate_dimensions().unwrap_err();
        assert!(err.to_string().contains("multiples of 16"));
    }

    #[test]
    fn test_validate_dimensions_zero_edge() {
        let p = ImageGenParams {
            prompt: "t".into(), quality: None,
            image_size: Some(ImageSizeValue::Custom { width: 0, height: 1080 }),
            size_tier: None, output_format: None, num_images: None,
            model_id: None,             image_urls: None,
            enable_safety_checker: None,
        };
        // Zero width gives pixel count 0 which is below minimum
        let err = p.validate_dimensions().unwrap_err();
        assert!(err.to_string().contains("pixel count"));
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

    fn route(
        provider: &str,
        generate: &str,
        edit: Option<&str>,
        tier: ImageTier,
        is_default: bool,
    ) -> ImageRoute {
        ImageRoute {
            provider_name: provider.to_string(),
            generate_model_id: generate.to_string(),
            edit_model_id: edit.map(|s| s.to_string()),
            tier,
            is_default,
        }
    }

    #[test]
    fn test_image_model_catalog_resolution() {
        let entries = vec![
            entry(
                "gptimage",
                "openai/gpt-image-2.6/flare/text-to-image",
                Some("openai/gpt-image-2.6/flare/edit"),
                "fal",
            ),
            entry(
                "seedream5pro",
                "bytedance-seed/seedream-5-0-pro",
                None,
                "openrouter",
            ),
        ];
        let catalog = ImageModelCatalog::new(entries, "gptimage");
        assert_eq!(
            catalog.resolve("seedream5pro"),
            Some(("bytedance-seed/seedream-5-0-pro", "openrouter"))
        );
        assert_eq!(
            catalog.resolve("gptimage"),
            Some(("openai/gpt-image-2.6/flare/text-to-image", "fal"))
        );
        assert_eq!(catalog.resolve("nope"), None);
        assert!(!catalog.is_empty());
    }

    #[test]
    fn test_image_model_catalog_mode_aware_resolution() {
        let catalog = ImageModelCatalog::new(
            vec![
                entry(
                    "gptimage",
                    "openai/gpt-image-2.6/flare/text-to-image",
                    Some("openai/gpt-image-2.6/flare/edit"),
                    "fal",
                ),
                entry(
                    "seedream5pro",
                    "bytedance-seed/seedream-5-0-pro",
                    None,
                    "openrouter",
                ),
            ],
            "gptimage",
        );
        // fal pair: edit mode swaps to the dedicated endpoint id (issue #100)
        assert_eq!(
            catalog.resolve_for_mode("gptimage", true),
            Some(("openai/gpt-image-2.6/flare/edit", "fal"))
        );
        // same-model editing: no companion → t2i id in both modes
        assert_eq!(
            catalog.resolve_for_mode("seedream5pro", true),
            Some(("bytedance-seed/seedream-5-0-pro", "openrouter"))
        );
        assert_eq!(
            catalog.resolve_for_mode("seedream5pro", false),
            Some(("bytedance-seed/seedream-5-0-pro", "openrouter"))
        );
    }

    #[test]
    fn test_image_model_catalog_sorted_aliases() {
        let catalog = ImageModelCatalog::new(
            vec![
                entry("seedream5pro", "a", None, "openrouter"),
                entry("banana", "b", None, "openrouter"),
                entry("qwen3", "c", None, "openrouter"),
            ],
            "qwen3",
        );
        assert_eq!(catalog.allowed_aliases(), vec!["banana", "qwen3", "seedream5pro"]);
        assert_eq!(catalog.valid_alias_list(), "banana, qwen3, seedream5pro");
        assert_eq!(catalog.referenced_providers(), vec!["openrouter"]);
    }

    #[test]
    fn test_image_model_catalog_default_alias() {
        let catalog = ImageModelCatalog::new(
            vec![entry("qwenimage", "qwen/qwen-image-2.1-pro", None, "openrouter")],
            "qwenimage",
        );
        assert_eq!(catalog.default_alias(), "qwenimage");
    }

    #[test]
    fn test_image_model_catalog_empty() {
        let catalog = ImageModelCatalog::new(Vec::new(), "gptimage");
        assert!(catalog.is_empty());
        assert_eq!(catalog.resolve("gptimage"), None);
        assert_eq!(catalog.valid_alias_list(), "(none configured)");
    }

    #[test]
    fn test_image_model_catalog_auto_aspect_capability() {
        // The predicate is model-id driven; the current roster has no fal
        // seedream/v5 route, but the capability still works for any future one.
        let seedream = ImageModelCatalog::new(
            vec![
                entry(
                    "synthetic-seedream",
                    "bytedance/seedream/v5/pro/text-to-image",
                    None,
                    "fal",
                ),
                entry(
                    "seedream5pro",
                    "bytedance-seed/seedream-5-0-pro",
                    None,
                    "openrouter",
                ),
            ],
            "seedream5pro",
        );
        assert!(seedream.supports_auto_aspect(), "seedream/v5 id → auto aspect supported");
        assert_eq!(
            seedream.resolve("synthetic-seedream"),
            Some(("bytedance/seedream/v5/pro/text-to-image", "fal"))
        );

        let flux = ImageModelCatalog::new(
            vec![entry("flux", "fal-ai/flux/schnell", None, "fal")],
            "flux",
        );
        assert!(!flux.supports_auto_aspect(), "non-seedream models → no auto aspect");

        let empty = ImageModelCatalog::new(Vec::new(), "gptimage");
        assert!(!empty.supports_auto_aspect());
    }

    #[test]
    fn test_image_model_traits_ming_design_model_id() {
        // Issue #104: the Ming Design model id is self-sized and
        // text-to-image only. The alias itself is no longer in the #114 roster,
        // but the wire-level traits remain model-id driven.
        let ming = ImageModelTraits::for_model_id("inclusionai/ming-image-0.1-design");
        assert!(ming.self_sized, "ming rejects explicit size params");
        assert!(ming.text_to_image_only, "ming accepts no input references");

        // Sibling layer model is not the registered model id — no false match.
        let layer = ImageModelTraits::for_model_id("inclusionai/ming-image-0.1-design-layer");
        assert!(!layer.text_to_image_only, "layer variant must not match the t2i marker");

        let other = ImageModelTraits::for_model_id("qwen/qwen-image-2.1-pro");
        assert_eq!(other, ImageModelTraits::default(), "unmarked models get all-default traits");
    }

    #[test]
    fn test_image_model_entry_traits() {
        let ming = entry("synthetic-ming", "inclusionai/ming-image-0.1-design", None, "openrouter");
        assert!(ming.traits().self_sized && ming.traits().text_to_image_only);
        let qwen = entry("qwenimage", "qwen/qwen-image-2.1-pro", None, "openrouter");
        assert!(!qwen.traits().self_sized && !qwen.traits().text_to_image_only);
    }

    // ----- Provider routes (issue #112) -----

    /// Synthetic two-provider alias exercising generic route ordering; the
    /// surviving #114 roster has no cross-provider routes.
    fn multi_route_entry() -> ImageModelEntry {
        ImageModelEntry {
            alias: "route-demo".to_string(),
            label: "Route Demo".to_string(),
            family: "demo".to_string(),
            version: "1".to_string(),
            deprecated: false,
            routes: vec![
                route(
                    "openrouter",
                    "demo/generate",
                    Some("demo/generate"),
                    ImageTier::Standard,
                    true,
                ),
                route(
                    "fal",
                    "demo/generate-fal",
                    Some("demo/edit-fal"),
                    ImageTier::Standard,
                    false,
                ),
            ],
        }
    }

    #[test]
    fn test_route_candidates_provider_pin() {
        let entry = multi_route_entry();
        let fal_only: Vec<&str> = entry
            .route_candidates(Some("fal"), None, false)
            .iter()
            .map(|r| r.provider_name.as_str())
            .collect();
        assert_eq!(fal_only, vec!["fal"]);
        assert!(entry.route_candidates(Some("replicate"), None, false).is_empty());
    }

    #[test]
    fn test_route_candidates_tier_filter() {
        let mut entry = multi_route_entry();
        entry.routes.push(route(
            "openrouter",
            "demo/turbo",
            None,
            ImageTier::Turbo,
            false,
        ));
        let turbo = entry.route_candidates(None, Some(ImageTier::Turbo), false);
        assert_eq!(turbo.len(), 1);
        assert_eq!(turbo[0].generate_model_id, "demo/turbo");
        assert!(entry.route_candidates(None, Some(ImageTier::Flash), false).is_empty());
    }

    #[test]
    fn test_edit_calls_prefer_routes_with_edit_endpoint() {
        let mut entry = multi_route_entry();
        entry.routes[0].edit_model_id = None; // openrouter generate-only
        let edit_order: Vec<&str> = entry
            .route_candidates(None, None, true)
            .iter()
            .map(|r| r.provider_name.as_str())
            .collect();
        assert_eq!(edit_order, vec!["fal", "openrouter"]);
        let generate_order: Vec<&str> = entry
            .route_candidates(None, None, false)
            .iter()
            .map(|r| r.provider_name.as_str())
            .collect();
        assert_eq!(generate_order, vec!["openrouter", "fal"]);
    }

    #[test]
    fn test_prefer_provider_reorders_default_alias() {
        let mut catalog = ImageModelCatalog::new(vec![multi_route_entry()], "route-demo");
        catalog.prefer_provider("fal");
        let entry = catalog.entry("route-demo").unwrap();
        assert_eq!(entry.default_route().provider_name, "fal");
        assert_eq!(entry.routes[0].provider_name, "fal");
        assert!(entry.routes[0].is_default);
        assert!(!entry.routes[1].is_default);
    }

    #[test]
    fn test_restricted_to_providers_drops_missing_backends() {
        let catalog = ImageModelCatalog::new(vec![multi_route_entry()], "route-demo");
        let available: std::collections::HashSet<&str> = ["openrouter"].into_iter().collect();
        let restricted = catalog.restricted_to_providers(&available);
        let entry = restricted.entry("route-demo").unwrap();
        assert_eq!(entry.providers(), vec!["openrouter"]);
        assert_eq!(restricted.referenced_providers(), vec!["openrouter"]);
    }
}