pub mod client;
pub mod config;
pub mod ddp;
pub mod error;
pub mod rest;
pub mod types;

pub use client::{MessageSender, RocketChatClient};
pub use config::{RocketChatConfig, ServerConfig};
pub use error::{Result, RocketChatError};
pub use rest::RestApiClient;
pub use types::{AttachmentInfo, BotReply, FileInfo, ImageDim, IncomingMessage, MessageFilter, MessageUrl, UrlHeaders};

/// Re-export of serde_json for convenience.
pub use serde_json;
