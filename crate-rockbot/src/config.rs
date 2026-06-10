use serde::Deserialize;
use std::collections::HashMap;
use webdav::WebDavConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub rocketchat: RocketChatSection,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
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

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub debug: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub default_provider: String,
    pub default_model: String,
    #[serde(default = "default_max_history_size")]
    pub max_history_size: usize,
    #[serde(default = "default_max_text_length")]
    pub max_text_length: usize,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

fn default_max_iterations() -> u32 {
    8
}

fn default_max_history_size() -> usize {
    12
}

fn default_max_text_length() -> usize {
    50000
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolServiceConfig {
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub base_url: String,
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
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_toml(content: &str) -> crate::error::Result<Self> {
        let config: Self = toml::from_str(content)?;
        Ok(config)
    }

    pub fn validate(&self) -> crate::error::Result<()> {
        let provider_name = &self.rocketchat.model.default_provider;
        self.find_provider(provider_name)
            .ok_or_else(|| crate::error::RockBotError::ProviderNotFound(provider_name.clone()))?;
        Ok(())
    }

    pub fn find_provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.name == name)
    }

    pub fn find_tool(&self, name: &str) -> Option<&ToolServiceConfig> {
        self.tools.get(name)
    }

    pub fn resolve_model(&self, provider_name: &str, model_alias: &str) -> Option<String> {
        let provider = self.find_provider(provider_name)?;
        provider.models.get(model_alias).cloned()
    }
}

impl ProviderConfig {
    pub fn chat_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        let path = self.chat_path.as_deref().unwrap_or("/chat/completions");
        format!("{}{}", base, path)
    }

    pub fn validate_api_key(&self) -> crate::error::Result<()> {
        if self.api_key.is_empty() || self.api_key == "EDITME" {
            return Err(crate::error::RockBotError::MissingApiKey(self.name.clone()));
        }
        Ok(())
    }
}
