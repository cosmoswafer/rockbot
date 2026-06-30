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
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {}
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
}

fn default_model_context_length() -> u32 {
    128_000
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
}

fn default_image_provider() -> ProviderName {
    ProviderName::try_new("openrouter".to_string()).expect("hardcoded default")
}
fn default_image_text_model() -> String {
    "mai".into()
}
fn default_image_edit_model() -> String {
    "mai".into()
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
        }
    }
}

fn default_max_iterations() -> u32 {
    47
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
    pub base_url: ConfigUrl,
    #[serde(default)]
    pub basecf_url: Option<String>,
    #[serde(default)]
    pub chat_path: Option<String>,
    #[serde(default)]
    pub draw_path: Option<String>,
    #[serde(default)]
    pub models: HashMap<String, String>,
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

        let config: Self = if raw.is_empty() {
            AppConfig::default()
        } else {
            toml::from_str(&raw)
                .map_err(|e| crate::error::RockBotError::Config(format!("toml parse: {}", e)))?
        };

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
        <Self as ValidatorValidate>::validate(&config).map_err(|e| {
            crate::error::RockBotError::Config(format!("config validation: {e}"))
        })?;
        Ok(config)
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

    Ok(())
}

impl ProviderConfig {
    pub fn chat_url(&self) -> String {
        let base = self.base_url.as_str().trim_end_matches('/');
        let path = self.chat_path.as_deref().unwrap_or("/chat/completions");
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
}
