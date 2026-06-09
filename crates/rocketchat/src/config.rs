use serde::{Deserialize, Serialize};

/// Configuration for connecting to a RocketChat server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocketChatConfig {
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub debug: bool,
    #[serde(default = "default_true")]
    pub use_tls: bool,
}

fn default_true() -> bool {
    true
}

impl RocketChatConfig {
    /// Load configuration from a TOML file path.
    pub fn from_file(path: &str) -> Result<Self, crate::error::RocketChatError> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
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
