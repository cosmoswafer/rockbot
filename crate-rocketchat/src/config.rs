use crate::validated::{Password, ServerUrl, Username};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocketChatConfig {
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: ServerUrl,
    pub username: Username,
    pub password: Password,
    #[serde(default = "default_true")]
    pub use_tls: bool,
}

fn default_true() -> bool {
    true
}

impl RocketChatConfig {
    /// Load configuration from a TOML file path.
    /// Expects the server config under a `[rocketchat.server]` section.
    pub fn from_file(path: &str) -> Result<Self, crate::error::RocketChatError> {
        let content = std::fs::read_to_string(path)?;
        let table: toml::Table = toml::from_str(&content)?;
        let rocketchat = table.get("rocketchat").ok_or_else(|| {
            crate::error::RocketChatError::MissingConfig(
                "missing [rocketchat] section in config".into(),
            )
        })?;
        let rc_str = toml::to_string(rocketchat)?;
        let config: Self = toml::from_str(&rc_str)?;
        Ok(config)
    }

    /// Build the WebSocket URI from the server URL.
    pub fn ws_uri(&self) -> Result<String, crate::error::RocketChatError> {
        let host = self.host();
        let protocol = if self.server.use_tls { "wss" } else { "ws" };
        Ok(format!("{}://{}/websocket", protocol, host))
    }

    /// Strip protocol prefix from the URL for use in TLS verification.
    pub fn host(&self) -> &str {
        self.server
            .url
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
    }
}
