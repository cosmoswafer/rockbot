use thiserror::Error;

#[derive(Error, Debug)]
pub enum RocketChatError {
    #[error("WebSocket error: {0}")]
    WebSocket(Box<tokio_tungstenite::tungstenite::Error>),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config error: {0}")]
    Config(#[from] toml::de::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Subscription failed: {0}")]
    SubscriptionFailed(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("TLS error: {0}")]
    Tls(String),
}

impl From<tokio_tungstenite::tungstenite::Error> for RocketChatError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        RocketChatError::WebSocket(Box::new(e))
    }
}

pub type Result<T> = std::result::Result<T, RocketChatError>;
