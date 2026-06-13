use serde::Deserialize;
use serde_valid::Validate;
use std::collections::HashMap;
use validator::Validate as ValidatorValidate;
use webdav::WebDavConfig;

use crate::validated::{BoundedUsize, ConfigUrl, ProviderName};

const DEFAULT_CONFIG_PATH: &str = "default.config.toml";

#[derive(Debug, Clone, Deserialize, ValidatorValidate)]
#[validate(schema(function = "validate_app_config"))]
pub struct AppConfig {
    pub rocketchat: RocketChatSection,
    #[serde(default)]
    pub chat_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub image_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub image_model: ImageModelConfig,
    #[serde(default)]
    pub tools: HashMap<String, ToolServiceConfig>,
    #[serde(default)]
    pub webdav: Option<WebDavConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RocketChatSection {
    pub server: ServerConfig,
    pub model: ModelConfig,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct ServerConfig {
    #[validate(min_length = 1)]
    pub url: String,
    #[validate(min_length = 1)]
    pub username: String,
    #[validate(min_length = 1)]
    pub password: String,
    #[serde(default)]
    pub debug: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub default_provider: ProviderName,
    pub default_model: String,
    #[serde(default = "default_max_history_size")]
    pub max_history_size: BoundedUsize,
    #[serde(default = "default_max_text_length")]
    pub max_text_length: BoundedUsize,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_summary_days")]
    pub summary_days: u32,
    #[serde(default = "default_max_summary_chars")]
    pub max_summary_chars: BoundedUsize,
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
    ProviderName::try_new("fal".to_string()).expect("hardcoded default")
}
fn default_image_text_model() -> String {
    "seedream".into()
}
fn default_image_edit_model() -> String {
    "fal-ai/nano-banana-pro/edit".into()
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
    28
}

fn default_max_history_size() -> BoundedUsize {
    BoundedUsize::try_new(18).expect("hardcoded default")
}

fn default_max_text_length() -> BoundedUsize {
    BoundedUsize::try_new(50000).expect("hardcoded default")
}

fn default_summary_days() -> u32 {
    3
}

fn default_max_summary_chars() -> BoundedUsize {
    BoundedUsize::try_new(8000).expect("hardcoded default")
}

fn default_max_soul_chars() -> BoundedUsize {
    BoundedUsize::try_new(2000).expect("hardcoded default")
}

fn default_persist_interval_secs() -> u64 {
    60
}

fn default_memory_ttl_secs() -> u64 {
    300
}

fn default_max_context_bytes() -> BoundedUsize {
    BoundedUsize::try_new(4_000_000).expect("hardcoded default")
}

fn default_max_attachment_bytes() -> u64 {
    25_000_000
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct ToolServiceConfig {
    #[validate(min_length = 1)]
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

impl AppConfig {
    pub fn from_file(path: &str) -> crate::error::Result<Self> {
        let default_raw = std::fs::read_to_string(DEFAULT_CONFIG_PATH)
            .map_err(|e| crate::error::RockBotError::Config(format!(
                "Failed to read default config ({}): {}. The install may be corrupt.",
                DEFAULT_CONFIG_PATH, e
            )))?;
        let default_value: toml::Value = toml::from_str(&default_raw)
            .map_err(|e| crate::error::RockBotError::Config(format!("default parse: {}", e)))?;

        let user_value: toml::Value = match std::fs::read_to_string(path) {
            Ok(raw) => toml::from_str(&raw)
                .map_err(|e| crate::error::RockBotError::Config(format!("user parse: {}", e)))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!("Config file '{}' not found, using defaults only", path);
                toml::Value::Table(toml::value::Table::new())
            }
            Err(e) => return Err(crate::error::RockBotError::Config(format!(
                "Failed to read config '{}': {}", path, e
            ))),
        };

        let merged = merge_toml(default_value, user_value);
        let merged_str = toml::to_string(&merged)
            .map_err(|e| crate::error::RockBotError::Config(format!("merge failed: {}", e)))?;
        let config: Self = toml::from_str(&merged_str)
            .map_err(|e| crate::error::RockBotError::Config(format!("merged parse: {}", e)))?;
        config.rocketchat.server.validate().map_err(|e| {
            crate::error::RockBotError::Config(format!("server config validation: {e}"))
        })?;
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
}

/// Validator schema function — cross-field business-logic validation for AppConfig.
fn validate_app_config(config: &AppConfig) -> Result<(), validator::ValidationError> {
    let provider_name: &str = &config.rocketchat.model.default_provider;
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
    Ok(())
}

/// Recursively merge two TOML values. `base` provides defaults, `override_` wins.
pub fn merge_toml(base: toml::Value, override_: toml::Value) -> toml::Value {
    match (base, override_) {
        (toml::Value::Table(mut base), toml::Value::Table(over)) => {
            for (k, v) in over {
                let merged = match base.remove(&k) {
                    Some(existing) => merge_toml(existing, v),
                    None => v,
                };
                base.insert(k, merged);
            }
            toml::Value::Table(base)
        }
        (toml::Value::Array(base_arr), toml::Value::Array(over_arr)) => {
            merge_named_arrays(base_arr, over_arr)
        }
        (_, over) => over,
    }
}

/// Merge arrays of tables by matching `name` key. User entries override defaults;
/// entries only in the default are kept; entries only in user config are appended.
fn merge_named_arrays(default: Vec<toml::Value>, user: Vec<toml::Value>) -> toml::Value {
    let mut merged: Vec<toml::Value> = default;
    for user_entry in user {
        let user_name = user_entry
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        if let Some(ref name) = user_name {
            if let Some(pos) = merged.iter().position(|e| {
                e.get("name").and_then(|n| n.as_str()) == Some(name)
            }) {
                // Merge user fields into the matching default entry
                let default_entry = merged.remove(pos);
                merged.push(merge_toml(default_entry, user_entry));
                continue;
            }
        }
        merged.push(user_entry);
    }
    toml::Value::Array(merged)
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
