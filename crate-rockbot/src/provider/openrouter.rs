use async_trait::async_trait;
use tracing::{debug, warn};

use crate::config::ProviderConfig;
use crate::error::{Result, RockBotError};
use crate::provider::AiProvider;
use crate::types::{ChatRequest, CompletionResult, FinishReason, ToolCall, UsageInfo};

pub struct OpenRouterProvider {
    api_key: String,
    base_url: String,
    model: String,
    #[allow(dead_code)]
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OpenRouterProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterProvider")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

impl OpenRouterProvider {
    pub fn new(config: &ProviderConfig, model: impl Into<String>) -> Result<Self> {
        config.validate_api_key()?;
        let api_key = config.api_key.clone();
        let full_url = config.chat_url();

        Ok(Self {
            api_key,
            base_url: full_url,
            model: model.into(),
            http_client: super::default_http_client(),
        })
    }

    pub fn with_client(
        config: &ProviderConfig,
        model: impl Into<String>,
        client: reqwest::Client,
    ) -> Result<Self> {
        config.validate_api_key()?;
        let api_key = config.api_key.clone();
        let full_url = config.chat_url();

        Ok(Self {
            api_key,
            base_url: full_url,
            model: model.into(),
            http_client: client,
        })
    }

    pub(crate) fn build_request_body(&self, request: &ChatRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": request.model,
            "messages": request.messages,
            "stream": request.stream,
        });

        if let Some(ref tools) = request.tools {
            body["tools"] = serde_json::to_value(tools).unwrap();
        }

        if let Some(ref tool_choice) = request.tool_choice {
            body["tool_choice"] = tool_choice.clone();
        }

        if let Some(ref thinking) = request.thinking {
            body["thinking"] = serde_json::json!({
                "type": thinking.thinking_type
            });
        }

        if let Some(ref effort) = request.reasoning_effort {
            body["reasoning_effort"] = serde_json::Value::String(effort.clone());
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if let Some(max_tok) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tok);
        }

        body
    }

    pub(crate) fn parse_response_body(body: &serde_json::Value) -> Result<CompletionResult> {
        let choices = body
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or(RockBotError::NoChoices)?;

        let choice = choices.first().ok_or(RockBotError::NoChoices)?;
        let message = choice.get("message").ok_or(RockBotError::EmptyResponse)?;

        let finish = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .map(|s| match s {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "tool_calls" => FinishReason::ToolUse,
                _ => FinishReason::Error,
            })
            .unwrap_or(FinishReason::Error);

        let text = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(String::from);

        let reasoning_content = message
            .get("reasoning_content")
            .and_then(|r| r.as_str())
            .map(String::from);

        let tool_calls: Vec<ToolCall> = message
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        serde_json::from_value::<ToolCall>(tc.clone())
                            .map_err(|e| {
                                warn!("openrouter: skipping malformed tool_call in response: {e}");
                            })
                            .ok()
                    })
                    .map(|mut tc| {
                        if serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                            .is_err()
                        {
                            tc.function.arguments = crate::provider::tool_args::repair_tool_args(
                                &tc.function.name,
                                &tc.function.arguments,
                            );
                        }
                        tc
                    })
                    .collect()
            })
            .unwrap_or_default();

        let usage = body.get("usage").and_then(|u| {
            Some(UsageInfo {
                prompt_tokens: u.get("prompt_tokens")?.as_u64()?,
                completion_tokens: u.get("completion_tokens")?.as_u64()?,
                total_tokens: u.get("total_tokens")?.as_u64()?,
            })
        });

        Ok(CompletionResult {
            text,
            tool_calls,
            finish,
            reasoning_content,
            usage,
        })
    }

    fn map_http_error(status: u16, body: &str) -> RockBotError {
        let msg = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| body.to_string());

        // Detect context-length exceeded errors at HTTP 400 level:
        // "This model's maximum context length is X tokens. However, you requested Y tokens..."
        if status == 400 && is_context_length_error(&msg) {
            return RockBotError::ContextLengthExceeded(msg);
        }

        match status {
            401 => RockBotError::AuthFailed(msg),
            429 => RockBotError::RateLimited { retry_after: None },
            500 | 502 | 503 => RockBotError::ServerError { status, body: msg },
            _ => RockBotError::Provider(format!("HTTP {}: {}", status, msg)),
        }
    }
}

#[async_trait]
impl AiProvider for OpenRouterProvider {
    async fn complete(&self, mut request: ChatRequest) -> Result<CompletionResult> {
        // Strip reasoning_content from messages: it's a response-only field that
        // some providers (e.g. Qwen) reject in request input.
        // Also sanitize tool_call arguments: Qwen may generate truncated/invalid
        // JSON in the arguments field (e.g. unterminated strings from length-limited
        // responses), which it then rejects when sent back in history.
        for msg in &mut request.messages {
            msg.reasoning_content = None;
        }
        crate::provider::tool_args::sanitize_messages_tool_calls(&mut request.messages);
        let body = self.build_request_body(&request);
        let msg_count = request.messages.len();
        let tool_count = request.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        debug!(
            "OpenRouter request: model={} messages={} tools={} stream={}",
            request.model, msg_count, tool_count, request.stream
        );
        let max_retries: u32 = 3;

        for attempt in 0..=max_retries {
            let response = self
                .http_client
                .post(&self.base_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .header("HTTP-Referer", "https://github.com/anomalyco/rockbot")
                .header("X-Title", "RockBot")
                .json(&body)
                .send()
                .await?;

            let status = response.status();
            if status.is_success() {
                let response_body: serde_json::Value = response.json().await?;
                let tool_count = response_body
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("tool_calls"))
                    .and_then(|t| t.as_array())
                    .map(|t| t.len())
                    .unwrap_or(0);
                let text_len = response_body
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0);
                debug!(
                    "OpenRouter response: finish={:?} text_len={} tool_calls={}",
                    response_body
                        .get("choices")
                        .and_then(|c| c.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|c| c.get("finish_reason"))
                        .and_then(|f| f.as_str())
                        .unwrap_or("unknown"),
                    text_len,
                    tool_count,
                );
                return Self::parse_response_body(&response_body);
            }

            let status_code = status.as_u16();
            let error_body = response.text().await.unwrap_or_default();

            if (status_code == 429 || status_code >= 500) && attempt < max_retries {
                let delay = 2u64.pow(attempt + 1);
                tracing::warn!(
                    "OpenRouter HTTP {}, retrying in {}s (attempt {}/{})",
                    status_code,
                    delay,
                    attempt + 1,
                    max_retries
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                continue;
            }

            tracing::warn!(
                "OpenRouter HTTP {} (non-retryable): error_body={} request_body={}",
                status_code,
                &error_body,
                serde_json::to_string(&body).unwrap_or_default()
            );

            return Err(Self::map_http_error(status_code, &error_body));
        }

        unreachable!("retry loop exhausted")
    }

    fn provider_name(&self) -> &str {
        "openrouter"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

/// Detect provider context-length exceeded errors from HTTP 400 error messages.
///
/// Matches patterns like:
/// "This model's maximum context length is 1048565 tokens. However, you requested 8618440 tokens..."
///
/// The check is: 400 status AND message contains "context length" (case-insensitive).
fn is_context_length_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("context length") || lower.contains("maximum context")
}

pub struct OpenRouterImageProvider {
    api_key: String,
    base_url: String,
    images_url: String,
    model: String,
    http_client: reqwest::Client,
    catalog_cache: tokio::sync::Mutex<Option<ImageCatalogCaps>>,
}

impl std::fmt::Debug for OpenRouterImageProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterImageProvider")
            .field("base_url", &self.base_url)
            .field("images_url", &self.images_url)
            .field("model", &self.model)
            .finish()
    }
}

/// Capability descriptor from the OpenRouter image catalog
/// (`supported_parameters` values; see `_dfd/ai/ai-provider.md` §3).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ParamDescriptor {
    Enum { values: Vec<String> },
    Range { min: i64, max: i64 },
    Boolean,
}

impl ParamDescriptor {
    fn enum_values(&self) -> Option<&[String]> {
        match self {
            ParamDescriptor::Enum { values } => Some(values),
            _ => None,
        }
    }

    fn range_max(&self) -> Option<i64> {
        match self {
            ParamDescriptor::Range { max, .. } => Some(*max),
            _ => None,
        }
    }
}

/// One entry of `GET {base}/images/models` → `data[]`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ImageCatalogModel {
    pub id: String,
    #[serde(default)]
    pub supported_parameters: Option<std::collections::HashMap<String, ParamDescriptor>>,
}

#[derive(Debug, serde::Deserialize)]
struct ImageCatalog {
    data: Vec<ImageCatalogModel>,
}

/// Domain caps derived from an `ImageCatalogModel` at the parse boundary.
#[derive(Debug, Clone, Default)]
pub struct ImageApiCaps {
    pub resolutions: Option<Vec<String>>,
    pub aspect_ratios: Option<Vec<String>>,
    pub n_max: Option<u32>,
    pub supports_quality: bool,
    pub supports_output_format: bool,
}

impl From<&ImageCatalogModel> for ImageApiCaps {
    fn from(model: &ImageCatalogModel) -> Self {
        let sp = model.supported_parameters.as_ref();
        let mut resolutions = sp
            .and_then(|m| m.get("resolution"))
            .and_then(|d| d.enum_values())
            .map(|values| {
                let mut sorted = values.to_vec();
                sorted.sort_by_key(|t| tier_rank(t));
                sorted
            });
        if matches!(resolutions.as_deref(), Some([])) {
            resolutions = None;
        }
        let mut aspect_ratios = sp
            .and_then(|m| m.get("aspect_ratio"))
            .and_then(|d| d.enum_values())
            .map(|values| values.to_vec());
        if matches!(aspect_ratios.as_deref(), Some([])) {
            aspect_ratios = None;
        }
        Self {
            resolutions,
            aspect_ratios,
            n_max: sp
                .and_then(|m| m.get("n"))
                .and_then(|d| d.range_max())
                .and_then(|max| u32::try_from(max).ok()),
            supports_quality: sp.is_some_and(|m| m.contains_key("quality")),
            supports_output_format: sp.is_some_and(|m| m.contains_key("output_format")),
        }
    }
}

/// Parsed image catalog: model id → caps.
type ImageCatalogCaps = std::collections::HashMap<String, ImageApiCaps>;

fn tier_rank(tier: &str) -> u8 {
    match tier {
        "512" => 0,
        "1K" => 1,
        "2K" => 2,
        "4K" => 3,
        _ => 2,
    }
}

impl OpenRouterImageProvider {
    pub fn new(config: &ProviderConfig, model: impl Into<String>) -> Result<Self> {
        config.validate_api_key()?;
        let full_url = config.chat_url();
        Ok(Self {
            api_key: config.api_key.clone(),
            base_url: full_url,
            images_url: config.images_url(),
            model: model.into(),
            http_client: super::default_http_client(),
            catalog_cache: tokio::sync::Mutex::new(None),
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model
    }

    pub fn provider_name(&self) -> &str {
        "openrouter"
    }

    fn preset_to_aspect_ratio(preset: &str) -> &str {
        match preset {
            "landscape_16_9" | "16:9" => "16:9",
            "portrait_16_9" | "9:16" => "9:16",
            "landscape_4_3" | "4:3" => "4:3",
            "portrait_4_3" | "3:4" => "3:4",
            "landscape_3_2" | "3:2" => "3:2",
            "portrait_2_3" | "2:3" => "2:3",
            "square" | "square_hd" | "1:1" => "1:1",
            _ => preset,
        }
    }
}

#[async_trait]
impl crate::provider::ImageProvider for OpenRouterImageProvider {
    async fn generate_image(&self, params: &crate::types::ImageGenParams) -> Result<Vec<u8>> {
        let model = params
            .model_id
            .clone()
            .unwrap_or_else(|| self.model.clone());
        let caps = self.catalog_caps().await;
        match caps.get(&model) {
            Some(caps) => self.generate_via_image_api(params, caps).await,
            None => self.generate_via_chat(params).await,
        }
    }

    async fn upload_file(&self, data: &[u8], content_type: &str) -> Result<String> {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data);
        Ok(format!("data:{};base64,{}", content_type, b64))
    }

    fn provider_name(&self) -> &str {
        "openrouter"
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

impl OpenRouterImageProvider {
    /// Fetch and cache the OpenRouter image catalog (`GET {images_url}/models`).
    /// Returns an empty map on fetch/parse failure so callers fall back to the
    /// legacy chat completions path.
    async fn catalog_caps(&self) -> ImageCatalogCaps {
        let mut cache = self.catalog_cache.lock().await;
        if let Some(map) = cache.as_ref() {
            return map.clone();
        }
        let url = format!("{}/models", self.images_url);
        let result = async {
            let response = self
                .http_client
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;
            let status = response.status();
            if !status.is_success() {
                let error_body = response.text().await.unwrap_or_default();
                return Err(RockBotError::Provider(format!(
                    "OpenRouter image catalog fetch failed (HTTP {}): {}",
                    status.as_u16(),
                    Self::extract_error_message(&error_body)
                )));
            }
            let catalog: ImageCatalog = response.json().await.map_err(|e| {
                RockBotError::Provider(format!("OpenRouter image catalog parse failed: {e}"))
            })?;
            Ok::<ImageCatalogCaps, RockBotError>(
                catalog
                    .data
                    .iter()
                    .map(|m| (m.id.clone(), ImageApiCaps::from(m)))
                    .collect(),
            )
        }
        .await;

        match result {
            Ok(map) => {
                debug!("OpenRouter image catalog loaded: {} models", map.len());
                *cache = Some(map.clone());
                map
            }
            Err(e) => {
                warn!("{e} — falling back to chat completions image path");
                std::collections::HashMap::new()
            }
        }
    }

    fn extract_error_message(error_body: &str) -> String {
        serde_json::from_str::<serde_json::Value>(error_body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| error_body.to_string())
    }

    fn resolve_resolution(requested: Option<&str>, caps: &ImageApiCaps) -> Option<String> {
        let allowed = caps.resolutions.as_deref()?;
        match requested {
            Some(r) if allowed.iter().any(|t| t == r) => Some(r.to_string()),
            Some(r) => {
                let rank = tier_rank(r);
                allowed
                    .iter()
                    .filter(|t| tier_rank(t) <= rank)
                    .max_by_key(|t| tier_rank(t))
                    .or_else(|| allowed.first())
                    .cloned()
            }
            None => allowed.last().cloned(),
        }
    }

    fn build_image_api_body(
        &self,
        params: &crate::types::ImageGenParams,
        caps: &ImageApiCaps,
    ) -> serde_json::Value {
        use crate::types::ImageSizeValue;

        let mut body = serde_json::Map::new();
        body.insert(
            "model".into(),
            serde_json::json!(params.model_id.as_deref().unwrap_or(&self.model)),
        );
        body.insert("prompt".into(), serde_json::json!(params.prompt));

        if let Some(resolution) = Self::resolve_resolution(params.size_tier.as_deref(), caps) {
            body.insert("resolution".into(), serde_json::json!(resolution));
        }

        if let Some(ref size) = params.image_size {
            let ratio = match size {
                ImageSizeValue::Preset(name) => Self::preset_to_aspect_ratio(name).to_string(),
                ImageSizeValue::Custom { width, height } => format!("{width}:{height}"),
            };
            let allowed = caps
                .aspect_ratios
                .as_ref()
                .is_none_or(|ratios| ratios.iter().any(|r| r == &ratio));
            if allowed {
                body.insert("aspect_ratio".into(), serde_json::json!(ratio));
            }
        }

        if let Some(n) = params.num_images {
            let clamped = caps.n_max.map(|max| n.min(max)).unwrap_or(n).max(1);
            body.insert("n".into(), serde_json::json!(clamped));
        }

        if let Some(ref quality) = params.quality
            && caps.supports_quality
        {
            body.insert("quality".into(), serde_json::json!(quality));
        }

        if let Some(ref format) = params.output_format
            && caps.supports_output_format
        {
            body.insert("output_format".into(), serde_json::json!(format));
        }

        if let Some(ref urls) = params.image_urls
            && !urls.is_empty()
        {
            let refs: Vec<serde_json::Value> = urls
                .iter()
                .map(|url| {
                    serde_json::json!({
                        "type": "image_url",
                        "image_url": { "url": url },
                    })
                })
                .collect();
            body.insert("input_references".into(), serde_json::Value::Array(refs));
        }

        serde_json::Value::Object(body)
    }

    /// Dedicated OpenRouter Image API path (`POST {images_url}`) for models
    /// present in the image catalog. See `_dfd/ai/ai-provider.md` §2d.
    async fn generate_via_image_api(
        &self,
        params: &crate::types::ImageGenParams,
        caps: &ImageApiCaps,
    ) -> Result<Vec<u8>> {
        let body_value = self.build_image_api_body(params, caps);

        debug!(
            "OpenRouter image api request: model={} prompt_len={} refs={} images_url={}",
            params.model_id.as_deref().unwrap_or(&self.model),
            params.prompt.len(),
            params.image_urls.as_ref().map(|u| u.len()).unwrap_or(0),
            self.images_url,
        );

        let response = self
            .http_client
            .post(&self.images_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/anomalyco/rockbot")
            .header("X-Title", "RockBot")
            .json(&body_value)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(RockBotError::Provider(format!(
                "OpenRouter image gen failed (HTTP {}): {}",
                status.as_u16(),
                Self::extract_error_message(&error_body)
            )));
        }

        let resp_body: serde_json::Value = response.json().await?;
        let data = resp_body
            .get("data")
            .and_then(|d| d.as_array())
            .filter(|d| !d.is_empty())
            .ok_or_else(|| {
                RockBotError::Provider("OpenRouter image gen: empty data array".into())
            })?;
        let b64 = data
            .first()
            .and_then(|d| d.get("b64_json"))
            .and_then(|b| b.as_str())
            .ok_or_else(|| {
                RockBotError::Provider("OpenRouter image gen: missing data[0].b64_json".into())
            })?;
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .map_err(|e| RockBotError::Provider(format!("OpenRouter image gen: base64 decode failed: {e}")))
    }

    /// Legacy chat completions path (`modalities: ["image"]`) — fallback for
    /// models absent from the image catalog.
    async fn generate_via_chat(
        &self,
        params: &crate::types::ImageGenParams,
    ) -> Result<Vec<u8>> {
        use crate::types::ImageSizeValue;

        let mut body = serde_json::Map::new();
        body.insert("model".into(), serde_json::json!(params.model_id.as_deref().unwrap_or(&self.model)));
        body.insert("stream".into(), serde_json::json!(false));

        // Build messages with image_urls if present (img2img)
        let user_content = if let Some(ref image_urls) = params.image_urls {
            if image_urls.is_empty() {
                serde_json::json!(&params.prompt)
            } else {
                let mut parts: Vec<serde_json::Value> = vec![serde_json::json!({
                    "type": "text",
                    "text": &params.prompt,
                })];
                for url in image_urls {
                    parts.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": { "url": url, "detail": "high" },
                    }));
                }
                serde_json::json!(parts)
            }
        } else {
            serde_json::json!(&params.prompt)
        };

        body.insert(
            "messages".into(),
            serde_json::json!([{ "role": "user", "content": user_content }]),
        );
        body.insert("modalities".into(), serde_json::json!(["image"]));

        // Build image_config
        let mut image_config = serde_json::Map::new();
        if let Some(ref size) = params.image_size {
            match size {
                ImageSizeValue::Preset(name) => {
                    image_config.insert(
                        "aspect_ratio".into(),
                        serde_json::json!(Self::preset_to_aspect_ratio(name)),
                    );
                }
                ImageSizeValue::Custom { width, height } => {
                    image_config.insert(
                        "aspect_ratio".into(),
                        serde_json::json!(format!("{}:{}", width, height)),
                    );
                }
            }
        }
        image_config.insert(
            "image_size".into(),
            serde_json::json!(params.size_tier.as_deref().unwrap_or("4K")),
        );
        if let Some(ref format) = params.output_format {
            image_config.insert("output_format".into(), serde_json::json!(format));
        }
        if let Some(ref quality) = params.quality {
            image_config.insert("quality".into(), serde_json::json!(quality));
        }
        if let Some(n) = params.num_images {
            image_config.insert("num_images".into(), serde_json::json!(n));
        }
        if !image_config.is_empty() {
            body.insert("image_config".into(), serde_json::Value::Object(image_config));
        }

        let body_value = serde_json::Value::Object(body);

        debug!(
            "OpenRouter image gen request: model={} prompt_len={} img2img={} base_url={}",
            params.model_id.as_deref().unwrap_or(&self.model),
            params.prompt.len(),
            params.image_urls.as_ref().map(|u| u.len()).unwrap_or(0),
            self.base_url,
        );

        let response = self
            .http_client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/anomalyco/rockbot")
            .header("X-Title", "RockBot")
            .json(&body_value)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(RockBotError::Provider(format!(
                "OpenRouter image gen failed (HTTP {}): {}",
                status.as_u16(),
                Self::extract_error_message(&error_body)
            )));
        }

        let resp_body: serde_json::Value = response.json().await?;
        let choices = resp_body
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or(RockBotError::NoChoices)?;
        let choice = choices.first().ok_or(RockBotError::NoChoices)?;
        let message = choice.get("message").ok_or(RockBotError::EmptyResponse)?;
        let images = message
            .get("images")
            .and_then(|imgs| imgs.as_array())
            .ok_or_else(|| RockBotError::Provider("OpenRouter image gen: no images in response".into()))?;
        let image = images
            .first()
            .ok_or_else(|| RockBotError::Provider("OpenRouter image gen: empty images array".into()))?;
        let data_url = image
            .get("image_url")
            .and_then(|iu| iu.get("url"))
            .and_then(|u| u.as_str())
            .ok_or_else(|| RockBotError::Provider("OpenRouter image gen: missing image_url.url".into()))?;

        // Parse data URI: data:image/png;base64,<data>
        let b64 = data_url
            .split_once(";base64,")
            .ok_or_else(|| RockBotError::Provider("OpenRouter image gen: non-base64 image URL".into()))?
            .1;
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .map_err(|e| RockBotError::Provider(format!("OpenRouter image gen: base64 decode failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, ThinkingConfig, ToolDef};
    use crate::validated::{ConfigUrl, ProviderName};

    fn make_provider(model: &str) -> OpenRouterProvider {
        let config = ProviderConfig {
            name: ProviderName::try_new("openrouter".to_string()).unwrap(),
            api_key: "sk-or-v1-test".into(),
            base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
            basecf_url: None,
            chat_path: Some("/chat/completions".into()),
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        OpenRouterProvider::new(&config, model).unwrap()
    }

    #[test]
    fn test_build_request_body_minimal() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Hello")],
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["model"], "openai/gpt-4");
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
        assert!(body.get("tools").is_none());
        assert!(body.get("temperature").is_none());
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn test_build_request_body_with_tools() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Weather?")],
            tools: Some(vec![ToolDef::new(
                "get_weather",
                "Get weather",
                serde_json::json!({"type": "object", "properties": {}}),
            )]),
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert!(body["tools"].is_array());
        assert_eq!(body["tools"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_build_request_body_with_temperature_and_max_tokens() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Hi")],
            tools: None,
            stream: false,
            temperature: Some(0.5),
            max_tokens: Some(1024),
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["max_tokens"], 1024);
    }

    #[test]
    fn test_build_request_body_with_thinking_enabled() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Think about it")],
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: Some(ThinkingConfig::enabled()),
            reasoning_effort: Some("medium".into()),
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "medium");
    }

    #[test]
    fn test_build_request_body_with_thinking_disabled() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("No think")],
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: Some(ThinkingConfig::disabled()),
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_build_request_body_with_tool_choice() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Force tool")],
            tools: Some(vec![ToolDef::new(
                "calc",
                "Calculator",
                serde_json::json!({"type": "object", "properties": {}}),
            )]),
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: Some(serde_json::json!("auto")),
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn test_parse_response_simple() {
        let json = serde_json::json!({
            "id": "or-123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello from OpenRouter!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let result = OpenRouterProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.text, Some("Hello from OpenRouter!".into()));
        assert_eq!(result.finish, FinishReason::Stop);
        assert_eq!(result.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn test_parse_response_with_tool_calls() {
        let json = serde_json::json!({
            "id": "or-456",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_001",
                        "type": "function",
                        "function": {
                            "name": "web_search",
                            "arguments": "{\"query\": \"rust\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = OpenRouterProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.finish, FinishReason::ToolUse);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].function.name, "web_search");
    }

    #[test]
    fn test_parse_response_with_reasoning() {
        let json = serde_json::json!({
            "id": "or-789",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "The answer is 42",
                    "reasoning_content": "Let me think..."
                },
                "finish_reason": "stop"
            }]
        });

        let result = OpenRouterProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.text, Some("The answer is 42".into()));
        assert_eq!(result.reasoning_content, Some("Let me think...".into()));
    }

    #[test]
    fn test_parse_response_length_finish() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Truncated..."
                },
                "finish_reason": "length"
            }]
        });

        let result = OpenRouterProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.finish, FinishReason::Length);
    }

    #[test]
    fn test_parse_response_no_choices() {
        let json = serde_json::json!({});
        let result = OpenRouterProvider::parse_response_body(&json);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_http_error_401() {
        let err = OpenRouterProvider::map_http_error(
            401,
            r#"{"error": {"message": "Invalid credentials"}}"#,
        );
        match err {
            RockBotError::AuthFailed(msg) => assert_eq!(msg, "Invalid credentials"),
            _ => panic!("Expected AuthFailed"),
        }
    }

    #[test]
    fn test_map_http_error_429() {
        let err = OpenRouterProvider::map_http_error(429, "");
        match err {
            RockBotError::RateLimited { .. } => {}
            _ => panic!("Expected RateLimited"),
        }
    }

    #[test]
    fn test_map_http_error_500() {
        let err = OpenRouterProvider::map_http_error(500, "Server boom");
        match err {
            RockBotError::ServerError { status, .. } => assert_eq!(status, 500),
            _ => panic!("Expected ServerError"),
        }
    }

    #[test]
    fn test_is_context_length_error_true() {
        let msg = "This model's maximum context length is 1048565 tokens. However, you requested 8618440 tokens (8614344 in the messages, 4096 in the completion). Please reduce the length of the messages or completion.";
        assert!(is_context_length_error(msg));
    }

    #[test]
    fn test_is_context_length_error_case_insensitive() {
        let msg = "CONTEXT LENGTH exceeded: model limit is X tokens";
        assert!(is_context_length_error(msg));
    }

    #[test]
    fn test_is_context_length_error_false() {
        assert!(!is_context_length_error("Invalid model name"));
        assert!(!is_context_length_error("Bad request: missing required field"));
    }

    #[test]
    fn test_map_http_error_context_length_exceeded() {
        let err = OpenRouterProvider::map_http_error(
            400,
            r#"{"error": {"message": "This model's maximum context length is 1048565 tokens. However, you requested 8618440 tokens."}}"#,
        );
        match err {
            RockBotError::ContextLengthExceeded(msg) => {
                assert!(msg.contains("maximum context length"));
            }
            other => panic!("Expected ContextLengthExceeded, got: {:?}", other),
        }
    }

    #[test]
    fn test_new_missing_api_key() {
        let config = ProviderConfig {
            name: ProviderName::try_new("openrouter".to_string()).unwrap(),
            api_key: "EDITME".into(),
            base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let result = OpenRouterProvider::new(&config, "openai/gpt-4");
        assert!(result.is_err());
    }

    #[test]
    fn test_new_empty_api_key() {
        let config = ProviderConfig {
            name: ProviderName::try_new("openrouter".to_string()).unwrap(),
            api_key: "".into(),
            base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let result = OpenRouterProvider::new(&config, "gpt");
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_name_and_model() {
        let provider = make_provider("openai/gpt-4o");
        assert_eq!(provider.provider_name(), "openrouter");
        assert_eq!(provider.model_name(), "openai/gpt-4o");
    }

    #[test]
    fn test_chat_url_custom_path() {
        let config = ProviderConfig {
            name: ProviderName::try_new("openrouter".to_string()).unwrap(),
            api_key: "sk-test".into(),
            base_url: ConfigUrl::try_new("https://custom.api.com".to_string()).unwrap(),
            basecf_url: None,
            chat_path: Some("/v2/chat".into()),
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let provider = OpenRouterProvider::new(&config, "model").unwrap();
        assert_eq!(provider.base_url, "https://custom.api.com/v2/chat");
    }

    #[test]
    fn test_with_client() {
        let config = ProviderConfig {
            name: ProviderName::try_new("openrouter".to_string()).unwrap(),
            api_key: "sk-test".into(),
            base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let client = reqwest::Client::new();
        let provider = OpenRouterProvider::with_client(&config, "openai/gpt-4", client).unwrap();
        assert_eq!(provider.model_name(), "openai/gpt-4");
    }

    mod image_provider {
        use super::super::*;
        use crate::provider::ImageProvider;
        use crate::types::ImageGenParams;
        use crate::validated::{ConfigUrl, ProviderName};

        fn make_image_provider(model: &str) -> OpenRouterImageProvider {
            let config = ProviderConfig {
                name: ProviderName::try_new("openrouter".to_string()).unwrap(),
                api_key: "sk-or-v1-test".into(),
                base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
                basecf_url: None,
                chat_path: Some("/chat/completions".into()),
                draw_path: None,
                models: std::collections::HashMap::new(),
            };
            OpenRouterImageProvider::new(&config, model).unwrap()
        }

        #[test]
        fn test_new_missing_api_key() {
            let config = ProviderConfig {
                name: ProviderName::try_new("openrouter".to_string()).unwrap(),
                api_key: "EDITME".into(),
                base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
                basecf_url: None,
                chat_path: None,
                draw_path: None,
                models: std::collections::HashMap::new(),
            };
            assert!(OpenRouterImageProvider::new(&config, "test-model").is_err());
        }

        #[test]
        fn test_new_empty_api_key() {
            let config = ProviderConfig {
                name: ProviderName::try_new("openrouter".to_string()).unwrap(),
                api_key: "".into(),
                base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
                basecf_url: None,
                chat_path: None,
                draw_path: None,
                models: std::collections::HashMap::new(),
            };
            assert!(OpenRouterImageProvider::new(&config, "test-model").is_err());
        }

        #[test]
        fn test_provider_names() {
            let provider = make_image_provider("google/gemini-3.1-flash-image-preview");
            assert_eq!(provider.provider_name(), "openrouter");
            assert_eq!(provider.model_id(), "google/gemini-3.1-flash-image-preview");
        }

        #[test]
        fn test_trait_provider_name() {
            let provider = make_image_provider("google/gemini-3.1-flash-image-preview");
            let trait_ref: &dyn ImageProvider = &provider;
            assert_eq!(trait_ref.provider_name(), "openrouter");
            assert_eq!(trait_ref.model_id(), "google/gemini-3.1-flash-image-preview");
        }

        #[test]
        fn test_preset_to_aspect_ratio_known() {
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("landscape_16_9"), "16:9");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("portrait_16_9"), "9:16");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("landscape_4_3"), "4:3");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("portrait_4_3"), "3:4");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("landscape_3_2"), "3:2");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("portrait_2_3"), "2:3");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("square"), "1:1");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("square_hd"), "1:1");
        }

        #[test]
        fn test_preset_to_aspect_ratio_passthrough() {
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("auto"), "auto");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("unknown"), "unknown");
        }

        #[test]
        fn test_preset_to_aspect_ratio_raw_strings() {
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("16:9"), "16:9");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("9:16"), "9:16");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("4:3"), "4:3");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("3:4"), "3:4");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("3:2"), "3:2");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("2:3"), "2:3");
            assert_eq!(OpenRouterImageProvider::preset_to_aspect_ratio("1:1"), "1:1");
        }

        #[tokio::test]
        async fn test_upload_file_returns_data_uri() {
            let provider = make_image_provider("google/gemini-3.1-flash-image-preview");
            let data = b"\x89PNG\r\n\x1a\nfake png bytes";
            let result = provider.upload_file(data, "image/png").await.unwrap();
            assert!(result.starts_with("data:image/png;base64,"));
            assert!(result.len() > "data:image/png;base64,".len());
        }

        #[test]
        fn test_parse_response_body_success() {
            let json = serde_json::json!({
                "id": "gen-abc123",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Here is an image.",
                        "images": [{
                            "type": "image_url",
                            "image_url": { "url": "data:image/png;base64,iVBORw0KGgo=" }
                        }]
                    },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 1, "total_tokens": 11 }
            });

            let result = OpenRouterProvider::parse_response_body(&json).unwrap();
            assert_eq!(result.text, Some("Here is an image.".into()));
            assert_eq!(result.finish, FinishReason::Stop);
            assert_eq!(result.usage.unwrap().total_tokens, 11);
        }

        #[test]
        fn test_image_params_model_id_override() {
            let mut params = ImageGenParams::new("test prompt");
            params.model_id = Some("black-forest-labs/flux.2-pro".into());
            assert_eq!(params.model_id.as_deref(), Some("black-forest-labs/flux.2-pro"));
        }

        fn qwen_catalog_entry() -> serde_json::Value {
            serde_json::json!({
                "id": "qwen/qwen-image-3-pro",
                "supported_parameters": {
                    "resolution": { "type": "enum", "values": ["2K", "1K"] },
                    "aspect_ratio": { "type": "enum", "values": ["1:1", "2:3", "16:9"] },
                    "n": { "type": "range", "min": 1, "max": 6 },
                    "input_references": { "type": "range", "min": 0, "max": 4 },
                    "seed": { "type": "boolean" }
                }
            })
        }

        #[test]
        fn test_param_descriptor_variants() {
            let e: ParamDescriptor =
                serde_json::from_value(serde_json::json!({"type": "enum", "values": ["1K", "2K"]})).unwrap();
            assert_eq!(e.enum_values(), Some(vec!["1K".to_string(), "2K".to_string()].as_slice()));
            let r: ParamDescriptor =
                serde_json::from_value(serde_json::json!({"type": "range", "min": 1, "max": 6})).unwrap();
            assert_eq!(r.range_max(), Some(6));
            let b: ParamDescriptor = serde_json::from_value(serde_json::json!({"type": "boolean"})).unwrap();
            assert!(matches!(b, ParamDescriptor::Boolean));
        }

        #[test]
        fn test_image_api_caps_from_qwen_catalog_entry() {
            let model: ImageCatalogModel = serde_json::from_value(qwen_catalog_entry()).unwrap();
            let caps = ImageApiCaps::from(&model);
            assert_eq!(caps.resolutions.as_deref(), Some(vec!["1K".to_string(), "2K".to_string()].as_slice()));
            assert_eq!(
                caps.aspect_ratios.as_deref(),
                Some(vec!["1:1".to_string(), "2:3".to_string(), "16:9".to_string()].as_slice())
            );
            assert_eq!(caps.n_max, Some(6));
            assert!(!caps.supports_quality);
            assert!(!caps.supports_output_format);
        }

        #[test]
        fn test_image_api_caps_quality_model() {
            let model: ImageCatalogModel = serde_json::from_value(serde_json::json!({
                "id": "openai/gpt-image-2",
                "supported_parameters": {
                    "resolution": { "type": "enum", "values": ["1K", "2K", "4K"] },
                    "quality": { "type": "enum", "values": ["low", "medium", "high"] },
                    "output_format": { "type": "enum", "values": ["png", "jpeg", "webp"] }
                }
            }))
            .unwrap();
            let caps = ImageApiCaps::from(&model);
            assert!(caps.supports_quality);
            assert!(caps.supports_output_format);
            assert_eq!(caps.n_max, None);
        }

        #[test]
        fn test_resolve_resolution_clamps_4k_down_to_2k() {
            let caps = ImageApiCaps {
                resolutions: Some(vec!["1K".into(), "2K".into()]),
                ..Default::default()
            };
            assert_eq!(
                OpenRouterImageProvider::resolve_resolution(Some("4K"), &caps),
                Some("2K".into())
            );
        }

        #[test]
        fn test_resolve_resolution_exact_match() {
            let caps = ImageApiCaps {
                resolutions: Some(vec!["1K".into(), "2K".into()]),
                ..Default::default()
            };
            assert_eq!(
                OpenRouterImageProvider::resolve_resolution(Some("1K"), &caps),
                Some("1K".into())
            );
        }

        #[test]
        fn test_resolve_resolution_below_min_picks_smallest() {
            let caps = ImageApiCaps {
                resolutions: Some(vec!["2K".into(), "4K".into()]),
                ..Default::default()
            };
            assert_eq!(
                OpenRouterImageProvider::resolve_resolution(Some("512"), &caps),
                Some("2K".into())
            );
        }

        #[test]
        fn test_resolve_resolution_none_picks_highest() {
            let caps = ImageApiCaps {
                resolutions: Some(vec!["1K".into(), "2K".into()]),
                ..Default::default()
            };
            assert_eq!(
                OpenRouterImageProvider::resolve_resolution(None, &caps),
                Some("2K".into())
            );
        }

        #[test]
        fn test_resolve_resolution_unsupported_omitted() {
            let caps = ImageApiCaps::default();
            assert_eq!(OpenRouterImageProvider::resolve_resolution(Some("4K"), &caps), None);
        }

        #[test]
        fn test_build_image_api_body_clamps_and_omits_unsupported() {
            let provider = make_image_provider("qwen/qwen-image-3-pro");
            let model: ImageCatalogModel = serde_json::from_value(qwen_catalog_entry()).unwrap();
            let caps = ImageApiCaps::from(&model);

            let mut params = ImageGenParams::new("a red panda");
            params.size_tier = Some("4K".into());
            params.image_size = Some(crate::types::ImageSizeValue::Preset("portrait_2_3".into()));
            params.num_images = Some(8);
            params.quality = Some("medium".into());
            params.output_format = Some("png".into());

            let body = provider.build_image_api_body(&params, &caps);
            assert_eq!(body["model"], "qwen/qwen-image-3-pro");
            assert_eq!(body["prompt"], "a red panda");
            assert_eq!(body["resolution"], "2K", "4K must clamp to highest supported tier");
            assert_eq!(body["aspect_ratio"], "2:3");
            assert_eq!(body["n"], 6, "num_images must clamp to n_max");
            assert!(body.get("quality").is_none(), "quality unsupported by qwen caps");
            assert!(body.get("output_format").is_none(), "output_format unsupported by qwen caps");
        }

        #[test]
        fn test_build_image_api_body_unsupported_ratio_omitted() {
            let provider = make_image_provider("qwen/qwen-image-3-pro");
            let model: ImageCatalogModel = serde_json::from_value(qwen_catalog_entry()).unwrap();
            let caps = ImageApiCaps::from(&model);

            let mut params = ImageGenParams::new("a red panda");
            params.image_size = Some(crate::types::ImageSizeValue::Preset("5:4".into()));

            let body = provider.build_image_api_body(&params, &caps);
            assert!(body.get("aspect_ratio").is_none(), "5:4 not in qwen aspect_ratio enum");
        }

        #[test]
        fn test_build_image_api_body_input_references() {
            let provider = make_image_provider("qwen/qwen-image-3-pro");
            let caps = ImageApiCaps::default();

            let mut params = ImageGenParams::new("make it watercolor");
            params.image_urls = Some(vec!["https://nc.example/s/abc".into()]);

            let body = provider.build_image_api_body(&params, &caps);
            assert_eq!(body["input_references"][0]["type"], "image_url");
            assert_eq!(body["input_references"][0]["image_url"]["url"], "https://nc.example/s/abc");
        }

        #[test]
        fn test_build_image_api_body_quality_model_includes_quality() {
            let provider = make_image_provider("openai/gpt-image-2");
            let caps = ImageApiCaps {
                supports_quality: true,
                supports_output_format: true,
                ..Default::default()
            };

            let mut params = ImageGenParams::new("a cat");
            params.quality = Some("high".into());
            params.output_format = Some("webp".into());
            params.num_images = Some(2);

            let body = provider.build_image_api_body(&params, &caps);
            assert_eq!(body["quality"], "high");
            assert_eq!(body["output_format"], "webp");
            assert_eq!(body["n"], 2);
        }
    }
}
