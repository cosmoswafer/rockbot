use serde::Deserialize;
use serde_valid::Validate;
use std::collections::HashMap;
use validator::Validate as ValidatorValidate;
use webdav::WebDavConfig;

use crate::validated::{BoundedUsize, ConfigUrl, ProviderName};

#[derive(Debug, Clone, Deserialize, ValidatorValidate)]
#[validate(schema(function = "validate_app_config"))]
pub struct AppConfig {
    #[serde(default)]
    pub platform: PlatformConfig,
    #[serde(default)]
    pub rocketchat: RocketChatSection,
    #[serde(default)]
    pub matrix: Option<MatrixSection>,
    #[serde(default)]
    pub model: ModelConfig,
    #[serde(default)]
    pub chat_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub image_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub image_model: ImageModelConfig,
    #[serde(default)]
    pub tools: HashMap<String, ToolServiceConfig>,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub webdav: Option<WebDavConfig>,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub acp: Option<AcpConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {}
    }
}

/// `[acp]` section — ACP (Agent Client Protocol) integration.
/// See `_dfd/tools/acp-delegate.md` (`AcpConfig` data structure).
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct AcpConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Executable on PATH or absolute path. Must be non-empty when `enabled = true`
    /// (cross-field rule enforced by `validate_app_config`).
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables for the child process. These are the ONLY
    /// variables passed besides `PATH`/`HOME` passthrough — the child never
    /// blanket-inherits rockbot's environment (secrets stay out of the agent).
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Child process working directory.
    #[serde(default = "default_acp_cwd")]
    pub cwd: String,
    /// `cwd` sent in `session/new` — the agent's workspace.
    #[serde(default = "default_acp_cwd")]
    pub session_cwd: String,
    /// Per-prompt timeout; triggers `session/cancel` on expiry.
    #[serde(default = "default_acp_timeout_secs")]
    #[validate(minimum = 10)]
    #[validate(maximum = 3600)]
    pub timeout_secs: u64,
    /// How to answer `session/request_permission` (default: deny).
    #[serde(default)]
    pub auto_approve_permissions: bool,
    /// Cap on aggregated agent output (protects the LLM context window).
    #[serde(default = "default_acp_max_response_chars")]
    pub max_response_chars: BoundedUsize,
}

fn default_acp_cwd() -> String {
    ".".into()
}

fn default_acp_timeout_secs() -> u64 {
    300
}

fn default_acp_max_response_chars() -> BoundedUsize {
    BoundedUsize::try_new(20000).expect("hardcoded default")
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: default_acp_cwd(),
            session_cwd: default_acp_cwd(),
            timeout_secs: default_acp_timeout_secs(),
            auto_approve_permissions: false,
            max_response_chars: default_acp_max_response_chars(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformConfig {
    #[serde(default = "default_platform_name")]
    pub name: String,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            name: default_platform_name(),
        }
    }
}

fn default_platform_name() -> String {
    "rocketchat".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct RocketChatSection {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub model: Option<ModelConfig>,
}

impl Default for RocketChatSection {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            model: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatrixSection {
    pub server: MatrixServerConfig,
    #[serde(default)]
    pub model: Option<ModelConfig>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct MatrixServerConfig {
    #[validate(min_length = 1)]
    pub homeserver: String,
    #[validate(min_length = 1)]
    pub user_id: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default = "default_matrix_state_dir")]
    pub state_dir: String,
}

fn default_matrix_state_dir() -> String {
    "./tmp/matrix-sdk".into()
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct ServerConfig {
    #[validate(min_length = 1)]
    #[serde(default)]
    pub url: String,
    #[validate(min_length = 1)]
    #[serde(default)]
    pub username: String,
    #[validate(min_length = 1)]
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub debug: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            username: String::new(),
            password: String::new(),
            debug: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub default_provider: ProviderName,
    pub default_model: String,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_max_soul_chars")]
    pub max_soul_chars: BoundedUsize,
    #[serde(default = "default_persist_interval_secs")]
    pub persist_interval_secs: u64,
    #[serde(default = "default_memory_ttl_secs")]
    pub memory_ttl_secs: u64,
    #[serde(default = "default_max_context_bytes")]
    pub max_context_bytes: BoundedUsize,
    #[serde(default = "default_max_attachment_bytes")]
    pub max_attachment_bytes: u64,
    #[serde(default = "default_model_context_length")]
    pub model_context_length: u32,
    #[serde(default = "default_summarization_enabled")]
    pub summarization_enabled: bool,
    #[serde(default = "default_summarization_ratio")]
    pub summarization_ratio: f64,
    #[serde(default = "default_summarization_target_tokens")]
    pub summarization_target_tokens: usize,
}

fn default_model_context_length() -> u32 {
    128_000
}

fn default_summarization_enabled() -> bool {
    true
}

fn default_summarization_ratio() -> f64 {
    0.6
}

fn default_summarization_target_tokens() -> usize {
    1024
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageModelConfig {
    #[serde(default = "default_image_provider")]
    pub default_provider: ProviderName,
    #[serde(default = "default_image_text_model")]
    pub default_text_model: String,
    #[serde(default = "default_image_edit_model")]
    pub default_edit_model: String,
    #[serde(default = "default_image_quality")]
    pub default_quality: String,
    #[serde(default = "default_image_output_format")]
    pub default_output_format: String,
    #[serde(default = "default_image_num_images")]
    pub default_num_images: u32,
    #[serde(default = "default_image_size")]
    pub default_image_size: String,
    #[serde(default = "default_image_size_tier")]
    pub default_image_size_tier: String,
    #[serde(default = "default_enable_safety_checker")]
    pub default_enable_safety_checker: bool,
}

fn default_image_provider() -> ProviderName {
    ProviderName::try_new("openrouter".to_string()).expect("hardcoded default")
}
fn default_image_text_model() -> String {
    "mai2pro".into()
}
fn default_image_edit_model() -> String {
    "mai2pro".into()
}
fn default_image_quality() -> String {
    "medium".into()
}

fn default_image_output_format() -> String {
    "png".into()
}

fn default_image_num_images() -> u32 {
    1
}

fn default_image_size() -> String {
    "portrait_2_3".into()
}

fn default_image_size_tier() -> String {
    "4K".into()
}

fn default_enable_safety_checker() -> bool {
    false
}

impl Default for ImageModelConfig {
    fn default() -> Self {
        Self {
            default_provider: default_image_provider(),
            default_text_model: default_image_text_model(),
            default_edit_model: default_image_edit_model(),
            default_quality: default_image_quality(),
            default_output_format: default_image_output_format(),
            default_num_images: default_image_num_images(),
            default_image_size: default_image_size(),
            default_image_size_tier: default_image_size_tier(),
            default_enable_safety_checker: default_enable_safety_checker(),
        }
    }
}

fn default_max_iterations() -> u32 {
    256
}

fn default_max_soul_chars() -> BoundedUsize {
    BoundedUsize::try_new(5000).expect("hardcoded default")
}

fn default_persist_interval_secs() -> u64 {
    120
}

fn default_memory_ttl_secs() -> u64 {
    600
}

fn default_max_context_bytes() -> BoundedUsize {
    BoundedUsize::try_new(4_000_000).expect("hardcoded default")
}

fn default_max_attachment_bytes() -> u64 {
    25_000_000
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default_provider: ProviderName::try_new("openrouter".to_string()).expect("hardcoded default"),
            default_model: "gpt".into(),
            max_iterations: default_max_iterations(),
            max_soul_chars: default_max_soul_chars(),
            persist_interval_secs: default_persist_interval_secs(),
            memory_ttl_secs: default_memory_ttl_secs(),
            max_context_bytes: default_max_context_bytes(),
            max_attachment_bytes: default_max_attachment_bytes(),
            model_context_length: default_model_context_length(),
            summarization_enabled: default_summarization_enabled(),
            summarization_ratio: default_summarization_ratio(),
            summarization_target_tokens: default_summarization_target_tokens(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct ToolServiceConfig {
    #[validate(min_length = 1)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_search_provider")]
    pub provider: String,
    #[serde(default)]
    pub exa: Option<ExaSearchConfig>,
    #[serde(default)]
    pub brave: Option<BraveSearchConfig>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            provider: default_search_provider(),
            exa: None,
            brave: None,
        }
    }
}

fn default_search_provider() -> String {
    "exa".into()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExaSearchConfig {
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BraveSearchConfig {
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub name: ProviderName,
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: ConfigUrl,
    #[serde(default)]
    pub basecf_url: Option<String>,
    #[serde(default)]
    pub chat_path: Option<String>,
    #[serde(default)]
    pub draw_path: Option<String>,
    #[serde(default)]
    pub models: HashMap<String, String>,
    /// Alias → dedicated edit-endpoint model id, for providers that genuinely
    /// use separate edit endpoints (currently only fal). Aliases absent from
    /// this map reuse their `models` id for editing (issue #100).
    #[serde(default)]
    pub edit_models: HashMap<String, String>,
}

fn default_base_url() -> ConfigUrl {
    ConfigUrl::try_new("http://localhost".to_string()).expect("hardcoded default")
}

/// Which provider role a `[[..._providers]]` entry serves. Role-scoped
/// default tables guarantee chat-only aliases (e.g. `gpt`) can never leak
/// into the image catalog or vice versa (issue #99).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRole {
    Chat,
    Image,
}

fn chat_default_models(kind: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    match kind {
        "openrouter" => {
            m.insert("gpt".to_string(), "openai/gpt-oss-120b:online".to_string());
            m.insert("qwen".to_string(), "qwen/qwen3.7-plus".to_string());
            m.insert("qwenflash".to_string(), "qwen/qwen3.7-flash".to_string());
            m.insert("minimax".to_string(), "minimax/minimax-m3".to_string());
            m.insert("mimo".to_string(), "xiaomi/mimo-v2.5".to_string());
            // Vision-capable chat models are also valid here.
            m.insert("seedream".to_string(), "bytedance-seed/seedream-4.5".to_string());
            m.insert("banana".to_string(), "google/gemini-3.1-flash-image-preview".to_string());
            m.insert("mai".to_string(), "microsoft/mai-image-2.5".to_string());
            m.insert("qwenimage".to_string(), "qwen/qwen-image-3-pro".to_string());
        }
        "deepseek" => {
            m.insert("flash".to_string(), "deepseek-v4-flash-vision-exp".to_string());
            m.insert("pro".to_string(), "deepseek-v4-pro".to_string());
        }
        "llamacpp" => {
            m.insert("local".to_string(), "local-model".to_string());
        }
        _ => {}
    }
    m
}

/// Image-role default tables — **image-capable models only** (issue #99):
/// no chat aliases. Edit companions exist only where the provider genuinely
/// hosts a separate edit endpoint — currently only fal (issue #100); ids
/// verified against the fal.ai public model registry.
fn image_default_models(kind: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    match kind {
        "openrouter" => {
            m.insert("seedream".to_string(), "bytedance-seed/seedream-4.5".to_string());
            m.insert("banana".to_string(), "google/gemini-3.1-flash-image-preview".to_string());
            // Issue #101: pro-only mai variant + new OpenRouter image models
            // (ids verified against GET /api/v1/images/models).
            m.insert("mai2pro".to_string(), "microsoft/mai-image-2.5-pro".to_string());
            m.insert("seedream5pro".to_string(), "bytedance-seed/seedream-5-0-pro".to_string());
            m.insert("grok2".to_string(), "x-ai/grok-imagine-image-2.0".to_string());
            m.insert("muse".to_string(), "meta/muse-image".to_string());
            m.insert("qwenimage".to_string(), "qwen/qwen-image-3-pro".to_string());
        }
        "fal" => {
            m.insert(
                "seedream".to_string(),
                "fal-ai/bytedance/seedream/v4.5/text-to-image".to_string(),
            );
            m.insert(
                "seedream5".to_string(),
                "bytedance/seedream/v5/pro/text-to-image".to_string(),
            );
            m.insert("gptimage".to_string(), "openai/gpt-image-2".to_string());
            m.insert(
                "grok".to_string(),
                "xai/grok-imagine-image/quality/text-to-image".to_string(),
            );
        }
        _ => {}
    }
    m
}

fn image_default_edit_models(kind: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if kind == "fal" {
        m.insert(
            "seedream5".to_string(),
            "bytedance/seedream/v5/pro/edit".to_string(),
        );
        m.insert("gptimage".to_string(), "openai/gpt-image-2/edit".to_string());
        m.insert(
            "grok".to_string(),
            "xai/grok-imagine-image/quality/edit".to_string(),
        );
    }
    m
}

fn provider_defaults() -> HashMap<String, ProviderConfig> {
    let mut map = HashMap::new();

    map.insert(
        "openrouter".to_string(),
        ProviderConfig {
            name: ProviderName::try_new("openrouter".to_string()).expect("hardcoded"),
            api_key: String::new(),
            base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string())
                .expect("hardcoded"),
            basecf_url: None,
            chat_path: Some("/chat/completions".to_string()),
            draw_path: Some("/images".to_string()),
            models: HashMap::new(),
            edit_models: HashMap::new(),
        },
    );

    map.insert(
        "deepseek".to_string(),
        ProviderConfig {
            name: ProviderName::try_new("deepseek".to_string()).expect("hardcoded"),
            api_key: String::new(),
            base_url: ConfigUrl::try_new("https://api.deepseek.com/v1".to_string())
                .expect("hardcoded"),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: HashMap::new(),
            edit_models: HashMap::new(),
        },
    );

    map.insert(
        "llamacpp".to_string(),
        ProviderConfig {
            name: ProviderName::try_new("llamacpp".to_string()).expect("hardcoded"),
            api_key: String::new(),
            base_url: ConfigUrl::try_new("http://localhost:8080/v1".to_string())
                .expect("hardcoded"),
            basecf_url: None,
            chat_path: Some("/chat/completions".to_string()),
            draw_path: None,
            models: HashMap::new(),
            edit_models: HashMap::new(),
        },
    );

    map.insert(
        "fal".to_string(),
        ProviderConfig {
            name: ProviderName::try_new("fal".to_string()).expect("hardcoded"),
            api_key: String::new(),
            base_url: ConfigUrl::try_new("https://queue.fal.run".to_string())
                .expect("hardcoded"),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: HashMap::new(),
            edit_models: HashMap::new(),
        },
    );

    map
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            platform: PlatformConfig::default(),
            rocketchat: RocketChatSection::default(),
            matrix: None,
            model: ModelConfig::default(),
            chat_providers: Vec::new(),
            image_providers: Vec::new(),
            image_model: ImageModelConfig::default(),
            tools: HashMap::new(),
            search: SearchConfig::default(),
            webdav: None,
            agent: AgentConfig::default(),
            acp: None,
        }
    }
}

impl AppConfig {
    pub fn from_file(path: &str) -> crate::error::Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!("Config file '{}' not found, using defaults only", path);
                String::new()
            }
            Err(e) => return Err(crate::error::RockBotError::Config(format!(
                "Failed to read config '{}': {}", path, e
            ))),
        };

        let mut config: Self = if raw.is_empty() {
            AppConfig::default()
        } else {
            toml::from_str(&raw)
                .map_err(|e| crate::error::RockBotError::Config(format!("toml parse: {}", e)))?
        };

        config.apply_provider_defaults();

        if config.platform.name == "rocketchat" {
            config.rocketchat.server.validate().map_err(|e| {
                crate::error::RockBotError::Config(format!("server config validation: {e}"))
            })?;
        }
        if config.platform.name == "matrix" {
            if let Some(ref mx) = config.matrix {
                mx.server.validate().map_err(|e| {
                    crate::error::RockBotError::Config(format!("matrix server config validation: {e}"))
                })?;
            }
        }
        if let Some(ref acp) = config.acp {
            acp.validate().map_err(|e| {
                crate::error::RockBotError::Config(format!("acp config validation: {e}"))
            })?;
        }
        <Self as ValidatorValidate>::validate(&config).map_err(|e| {
            crate::error::RockBotError::Config(format!("config validation: {e}"))
        })?;
        Ok(config)
    }

    fn apply_provider_defaults(&mut self) {
        for p in &mut self.chat_providers {
            Self::fill_provider_defaults(p, ProviderRole::Chat);
        }
        for p in &mut self.image_providers {
            Self::fill_provider_defaults(p, ProviderRole::Image);
        }
    }

    fn fill_provider_defaults(p: &mut ProviderConfig, role: ProviderRole) {
        const SENTINEL: &str = "http://localhost";
        if p.base_url.as_str() == SENTINEL {
            if let Some(defaults) = provider_defaults().get(p.name.as_str()) {
                p.base_url = defaults.base_url.clone();
                if p.models.is_empty() {
                    p.models = match role {
                        // Role-scoped tables (issue #99): chat aliases and
                        // image aliases live in disjoint default maps.
                        ProviderRole::Chat => chat_default_models(p.name.as_str()),
                        ProviderRole::Image => image_default_models(p.name.as_str()),
                    };
                }
                if p.edit_models.is_empty() {
                    p.edit_models = image_default_edit_models(p.name.as_str());
                }
                if p.chat_path.is_none() {
                    p.chat_path = defaults.chat_path.clone();
                }
                if p.draw_path.is_none() {
                    p.draw_path = defaults.draw_path.clone();
                }
            }
        }
    }

    pub fn from_toml(content: &str) -> crate::error::Result<Self> {
        let config: Self = toml::from_str(content)
            .map_err(|e| crate::error::RockBotError::Config(format!("toml parse: {}", e)))?;
        Ok(config)
    }

    pub fn find_chat_provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.chat_providers.iter().find(|p| p.name.as_str() == name)
    }

    pub fn find_image_provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.image_providers.iter().find(|p| p.name.as_str() == name)
    }

    pub fn resolve_chat_model(&self, provider_name: &str, model_alias: &str) -> Option<String> {
        let provider = self.find_chat_provider(provider_name)?;
        provider.models.get(model_alias).cloned()
    }

    pub fn resolve_image_model(&self, provider_name: &str, model_alias: &str) -> Option<String> {
        let provider = self.find_image_provider(provider_name)?;
        provider.models.get(model_alias).cloned()
    }

    /// Returns the platform-specific model config based on the active platform.
    /// Returns the Exa API key, checking [search.exa] first, then falling back to legacy [tools.exa].
    pub fn search_api_key(&self) -> String {
        if let Some(ref exa) = self.search.exa {
            if !exa.api_key.is_empty() {
                return exa.api_key.clone();
            }
        }
        self.tools.get("exa").map(|t| t.api_key.clone()).unwrap_or_default()
    }

    /// Returns the Brave Search API key from [search.brave].
    pub fn brave_api_key(&self) -> String {
        self.search.brave.as_ref().map(|b| b.api_key.clone()).unwrap_or_default()
    }

    pub fn active_model(&self) -> &ModelConfig {
        if self.platform.name == "rocketchat" {
            self.rocketchat.model.as_ref().unwrap_or(&self.model)
        } else if self.platform.name == "matrix" {
            self.matrix.as_ref().and_then(|mx| mx.model.as_ref()).unwrap_or(&self.model)
        } else {
            &self.model
        }
    }
}

/// Validator schema function — cross-field business-logic validation for AppConfig.
fn validate_app_config(config: &AppConfig) -> Result<(), validator::ValidationError> {
    let provider_name: &str = &config.active_model().default_provider;
    if config.find_chat_provider(provider_name).is_none() {
        let mut err = validator::ValidationError::new("provider_not_found");
        err.message = Some(format!("chat_provider '{}' not found in [[chat_providers]]", provider_name).into());
        return Err(err);
    }

    let image_provider: &str = &config.image_model.default_provider;
    if !config.image_providers.is_empty() {
        if config.find_image_provider(image_provider).is_none() {
            let mut err = validator::ValidationError::new("provider_not_found");
            err.message = Some(format!("image_provider '{}' not found in [[image_providers]]", image_provider).into());
            return Err(err);
        }
    }

    match config.platform.name.as_str() {
        "rocketchat" => {}
        "matrix" => {
            if config.matrix.is_none() {
                let mut err = validator::ValidationError::new("matrix_missing");
                err.message = Some("[matrix.server] section required when platform.name = \"matrix\"".into());
                return Err(err);
            }
        }
        other => {
            let mut err = validator::ValidationError::new("invalid_platform");
            err.message = Some(format!("platform.name must be \"rocketchat\" or \"matrix\", got \"{}\"", other).into());
            return Err(err);
        }
    }

    if let Some(ref acp) = config.acp {
        if acp.enabled && acp.command.trim().is_empty() {
            let mut err = validator::ValidationError::new("acp_command_missing");
            err.message = Some("[acp] command must be non-empty when enabled = true".into());
            return Err(err);
        }
    }

    Ok(())
}

impl ProviderConfig {
    pub fn chat_url(&self) -> String {
        let base = self.base_url.as_str().trim_end_matches('/');
        let path = self.chat_path.as_deref().unwrap_or("/chat/completions");
        format!("{}{}", base, path)
    }

    pub fn images_url(&self) -> String {
        let base = self.base_url.as_str().trim_end_matches('/');
        let path = self.draw_path.as_deref().unwrap_or("/images");
        format!("{}{}", base, path)
    }

    pub fn validate_api_key(&self) -> crate::error::Result<()> {
        if self.api_key.is_empty() || self.api_key == "EDITME" {
            return Err(crate::error::RockBotError::MissingApiKey(self.name.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_defaults_are_role_scoped_no_chat_models() {
        // Issue #99: image providers inherit an image-only default table;
        // chat aliases like 'gpt' must never leak into the image catalog,
        // while the chat role keeps them.
        let toml_str = r#"
[[chat_providers]]
name = "openrouter"
api_key = "k"

[[image_providers]]
name = "openrouter"
api_key = "k"
"#;
        let mut config = AppConfig::from_toml(toml_str).unwrap();
        config.apply_provider_defaults();

        let img = config.find_image_provider("openrouter").unwrap();
        assert!(
            !img.models.contains_key("gpt"),
            "chat alias 'gpt' leaked into image defaults (#99)"
        );
        assert!(!img.models.contains_key("qwen"));
        assert!(!img.models.contains_key("minimax"));
        assert!(img.models.contains_key("banana"));
        assert!(img.models.contains_key("mai2pro"));

        let chat = config.find_chat_provider("openrouter").unwrap();
        assert_eq!(
            chat.models.get("gpt").map(|s| s.as_str()),
            Some("openai/gpt-oss-120b:online"),
            "chat role keeps the full default table"
        );
    }

    #[test]
    fn test_fal_defaults_unified_aliases_with_edit_companions() {
        // Issue #100: one alias per model family; dedicated edit endpoints
        // live in edit_models (fal only) instead of separate *_edit aliases.
        let toml_str = r#"
[[image_providers]]
name = "fal"
api_key = "k"

[[image_providers]]
name = "openrouter"
api_key = "k"
"#;
        let mut config = AppConfig::from_toml(toml_str).unwrap();
        config.apply_provider_defaults();

        let fal = config.find_image_provider("fal").unwrap();
        assert!(fal.models.contains_key("seedream5"), "unified alias");
        assert!(fal.models.contains_key("gptimage"), "unified alias");
        assert!(fal.models.contains_key("grok"), "base grok alias now exists");
        assert!(
            !fal.models.keys().any(|a| a.ends_with("_edit")),
            "no selectable *_edit aliases: {:?}",
            fal.models.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            fal.edit_models.get("seedream5").map(|s| s.as_str()),
            Some("bytedance/seedream/v5/pro/edit")
        );
        assert_eq!(
            fal.edit_models.get("grok").map(|s| s.as_str()),
            Some("xai/grok-imagine-image/quality/edit")
        );

        // Non-fal providers get no edit companions — same model both modes
        let or = config.find_image_provider("openrouter").unwrap();
        assert!(or.edit_models.is_empty(), "openrouter edits reuse the t2i id");
    }

    #[test]
    fn test_openrouter_image_defaults_issue101_aliases() {
        // Issue #101: new OpenRouter image models (ids verified against
        // GET /api/v1/images/models); the non-pro mai alias is removed.
        let toml_str = r#"
[[image_providers]]
name = "openrouter"
api_key = "k"

[image_model]
default_provider = "openrouter"
"#;
        let mut config = AppConfig::from_toml(toml_str).unwrap();
        config.apply_provider_defaults();

        let or = config.find_image_provider("openrouter").unwrap();
        assert_eq!(
            or.models.get("mai2pro").map(|s| s.as_str()),
            Some("microsoft/mai-image-2.5-pro"),
            "pro variant replaces the deprecated non-pro alias"
        );
        assert_eq!(
            or.models.get("seedream5pro").map(|s| s.as_str()),
            Some("bytedance-seed/seedream-5-0-pro")
        );
        assert_eq!(
            or.models.get("grok2").map(|s| s.as_str()),
            Some("x-ai/grok-imagine-image-2.0")
        );
        assert_eq!(
            or.models.get("muse").map(|s| s.as_str()),
            Some("meta/muse-image")
        );
        assert!(
            !or.models.contains_key("mai"),
            "non-pro mai alias must be removed (issue #101)"
        );

        // [image_model] defaults point at the surviving pro alias
        assert_eq!(config.image_model.default_text_model, "mai2pro");
        assert_eq!(config.image_model.default_edit_model, "mai2pro");
    }

    fn make_base_config() -> String {
        r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"

[[chat_providers]]
name = "openrouter"
api_key = "sk-test"
base_url = "https://openrouter.ai/api/v1"

[[chat_providers]]
name = "deepseek"
api_key = "sk-test"
base_url = "https://api.deepseek.com/v1"
"#.to_string()
    }

    #[test]
    fn test_active_model_uses_rocketchat_model_when_present() {
        let toml_str = make_base_config() + r#"
[rocketchat.model]
default_provider = "deepseek"
default_model = "flash"

[model]
default_provider = "openrouter"
default_model = "gpt"
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        let active = config.active_model();
        assert_eq!(active.default_provider.as_str(), "deepseek");
        assert_eq!(active.default_model, "flash");
    }

    #[test]
    fn test_active_model_falls_back_when_rocketchat_model_is_absent() {
        let toml_str = make_base_config() + r#"
[model]
default_provider = "openrouter"
default_model = "gpt"
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        assert!(config.rocketchat.model.is_none());
        let active = config.active_model();
        assert_eq!(active.default_provider.as_str(), "openrouter");
        assert_eq!(active.default_model, "gpt");
    }

    #[test]
    fn test_toml_parses_rocketchat_model() {
        let toml_str = make_base_config() + r#"
[rocketchat.model]
default_provider = "deepseek"
default_model = "flash"
max_iterations = 10
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        assert!(config.rocketchat.model.is_some());
        let m = config.rocketchat.model.as_ref().unwrap();
        assert_eq!(m.default_provider.as_str(), "deepseek");
        assert_eq!(m.default_model, "flash");
        assert_eq!(m.max_iterations, 10);
    }

    #[test]
    fn test_toml_top_level_model_still_works() {
        let toml_str = make_base_config() + r#"
[model]
default_provider = "openrouter"
default_model = "gpt"
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        assert!(config.rocketchat.model.is_none());
        let active = config.active_model();
        assert_eq!(active.default_provider.as_str(), "openrouter");
        assert_eq!(active.default_model, "gpt");
    }

    // ─── ACP config tests ────────────────────────────────────────────────────

    #[test]
    fn test_acp_absent_by_default() {
        let toml_str = make_base_config() + r#"
[model]
default_provider = "openrouter"
default_model = "gpt"
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        assert!(config.acp.is_none());
    }

    #[test]
    fn test_acp_disabled_parses_without_command() {
        let toml_str = make_base_config() + r#"
[acp]
enabled = false
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        let acp = config.acp.as_ref().unwrap();
        assert!(!acp.enabled);
        assert!(acp.command.is_empty());
        // Cross-field rule must not fire when disabled.
        <AppConfig as ValidatorValidate>::validate(&config).unwrap();
    }

    #[test]
    fn test_acp_enabled_with_empty_command_fails_validation() {
        let toml_str = make_base_config() + r#"
[acp]
enabled = true
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        let err = <AppConfig as ValidatorValidate>::validate(&config).unwrap_err();
        assert!(
            err.to_string().contains("command must be non-empty when enabled = true"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_acp_enabled_with_blank_command_fails_validation() {
        let toml_str = make_base_config() + r#"
[acp]
enabled = true
command = "   "
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        let err = <AppConfig as ValidatorValidate>::validate(&config).unwrap_err();
        assert!(
            err.to_string().contains("command must be non-empty when enabled = true"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_acp_enabled_with_command_passes_validation() {
        let toml_str = make_base_config() + r#"
[acp]
enabled = true
command = "deno"
args = ["x", "opencode-ai", "acp"]
session_cwd = "/tmp/work"
timeout_secs = 120
auto_approve_permissions = true
max_response_chars = 5000

[acp.env]
FOO = "bar"
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        let acp = config.acp.as_ref().unwrap();
        assert!(acp.enabled);
        assert_eq!(acp.command, "deno");
        assert_eq!(acp.args, vec!["x", "opencode-ai", "acp"]);
        assert_eq!(acp.env.get("FOO").unwrap(), "bar");
        assert_eq!(acp.cwd, ".");
        assert_eq!(acp.session_cwd, "/tmp/work");
        assert_eq!(acp.timeout_secs, 120);
        assert!(acp.auto_approve_permissions);
        assert_eq!(acp.max_response_chars.as_usize(), 5000);
        <AppConfig as ValidatorValidate>::validate(&config).unwrap();
        acp.validate().unwrap();
    }

    #[test]
    fn test_acp_defaults_applied() {
        let toml_str = make_base_config() + r#"
[acp]
enabled = true
command = "codex-acp"
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        let acp = config.acp.as_ref().unwrap();
        assert_eq!(acp.args, Vec::<String>::new());
        assert!(acp.env.is_empty());
        assert_eq!(acp.cwd, ".");
        assert_eq!(acp.session_cwd, ".");
        assert_eq!(acp.timeout_secs, 300);
        assert!(!acp.auto_approve_permissions);
        assert_eq!(acp.max_response_chars.as_usize(), 20000);
        acp.validate().unwrap();
    }

    #[test]
    fn test_acp_timeout_out_of_bounds_fails_field_validation() {
        let toml_str = make_base_config() + r#"
[acp]
enabled = true
command = "codex-acp"
timeout_secs = 5
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        assert!(config.acp.as_ref().unwrap().validate().is_err());

        let toml_str = make_base_config() + r#"
[acp]
enabled = true
command = "codex-acp"
timeout_secs = 3601
"#;
        let config = AppConfig::from_toml(&toml_str).unwrap();
        assert!(config.acp.as_ref().unwrap().validate().is_err());
    }
}
