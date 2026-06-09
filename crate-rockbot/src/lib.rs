pub mod config;
pub mod error;
pub mod provider;
pub mod types;

pub use config::AppConfig;
pub use error::{Result, RockBotError};
pub use provider::{AiProvider, DeepSeekProvider, OpenRouterProvider};
pub use types::{
    ChatMessage, ChatRequest, CompletionResult, ContentPart, FinishReason, MessageContent, Role,
    ToolCall, ToolDef,
};
