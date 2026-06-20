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
    Config(String),

    #[error("Missing config section: {0}")]
    MissingConfig(String),

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

    #[error("WebSocket read timeout after {0}s")]
    ReadTimeout(u64),

    #[error("No application messages received for {0}s (DDP subscription may be dead)")]
    AppActivityTimeout(u64),

    #[error("Setup phase timed out after {0}s")]
    SetupTimeout(u64),
}

impl From<tokio_tungstenite::tungstenite::Error> for RocketChatError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        RocketChatError::WebSocket(Box::new(e))
    }
}

impl From<toml::de::Error> for RocketChatError {
    fn from(e: toml::de::Error) -> Self {
        RocketChatError::Config(e.to_string())
    }
}

impl From<toml::ser::Error> for RocketChatError {
    fn from(e: toml::ser::Error) -> Self {
        RocketChatError::Config(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, RocketChatError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_activity_timeout_display() {
        let err = RocketChatError::AppActivityTimeout(1800);
        assert_eq!(
            err.to_string(),
            "No application messages received for 1800s (DDP subscription may be dead)"
        );
    }

    #[test]
    fn test_setup_timeout_display() {
        let err = RocketChatError::SetupTimeout(60);
        assert_eq!(
            err.to_string(),
            "Setup phase timed out after 60s"
        );
    }

    #[test]
    fn test_read_timeout_display() {
        let err = RocketChatError::ReadTimeout(300);
        assert_eq!(
            err.to_string(),
            "WebSocket read timeout after 300s"
        );
    }

    #[test]
    fn test_app_activity_timeout_variant_equality() {
        let err1 = RocketChatError::AppActivityTimeout(1800);
        let err2 = RocketChatError::AppActivityTimeout(1800);
        assert_eq!(err1.to_string(), err2.to_string());
    }

    #[test]
    fn test_error_variants_are_different() {
        let app = RocketChatError::AppActivityTimeout(1800);
        let setup = RocketChatError::SetupTimeout(60);
        assert_ne!(app.to_string(), setup.to_string());
    }
}
