use thiserror::Error;

#[derive(Error, Debug)]
pub enum WebDavError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Directory already exists: {0}")]
    AlreadyExists(String),

    #[error("Path is not a directory: {0}")]
    NotADirectory(String),

    #[error("XML parse error: {0}")]
    XmlParse(String),

    #[error("Unexpected HTTP status {status}: {body}")]
    UnexpectedStatus { status: u16, body: String },

    #[error("Serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("Base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    #[error("XML deserialization error: {0}")]
    XmlDe(#[from] quick_xml::DeError),
}

pub type Result<T> = std::result::Result<T, WebDavError>;
