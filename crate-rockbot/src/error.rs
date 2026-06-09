use thiserror::Error;

#[derive(Error, Debug)]
pub enum RockBotError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Config error: {0}")]
    Config(#[from] toml::de::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Rate limited (429): retry after {retry_after:?}")]
    RateLimited { retry_after: Option<u64> },

    #[error("Server error ({status}): {body}")]
    ServerError { status: u16, body: String },

    #[error("Insufficient balance (402)")]
    InsufficientBalance,

    #[error("Invalid parameters (422): {0}")]
    InvalidParameters(String),

    #[error("Invalid request format (400): {0}")]
    InvalidRequest(String),

    #[error("Missing API key for provider '{0}'")]
    MissingApiKey(String),

    #[error("Provider '{0}' not found in config")]
    ProviderNotFound(String),

    #[error("Model '{model}' not found for provider '{provider}'")]
    ModelNotFound { provider: String, model: String },

    #[error("No API response choices returned")]
    NoChoices,

    #[error("Empty response content from provider")]
    EmptyResponse,

    #[error("Tool call parse error: {0}")]
    ToolCallParse(String),
}

pub type Result<T> = std::result::Result<T, RockBotError>;
