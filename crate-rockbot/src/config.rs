use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub rocketchat: RocketChatSection,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RocketChatSection {
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub debug: bool,
    pub default_provider: String,
    pub default_model: String,
    #[serde(default)]
    pub tools: bool,
    #[serde(default = "default_max_history_size")]
    pub max_history_size: usize,
    #[serde(default = "default_max_text_length")]
    pub max_text_length: usize,
}

fn default_max_history_size() -> usize {
    12
}

fn default_max_text_length() -> usize {
    50000
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
        Ok(config)
    }

    pub fn from_str(content: &str) -> crate::error::Result<Self> {
        let config: Self = toml::from_str(content)?;
        Ok(config)
    }

    pub fn find_provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.name == name)
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
}
