pub mod config;
pub mod error;
pub mod harness;
pub mod knowledge;
pub mod memory;
pub mod provider;
pub mod tool;
pub mod tools;
pub mod types;
pub mod utils;

pub use config::{merge_toml, AppConfig, ProviderConfig};
pub use error::{Result, RockBotError};
pub use harness::AgentHarness;
pub use memory::{ConversationHistory, MemoryManager, RoomState};
pub use provider::{AiProvider, DeepSeekProvider, FalAiProvider, ImageProvider, OpenRouterProvider};
pub use tool::{Tool, ToolRegistry, ToolResult};
pub use types::{
    ChatMessage, ChatRequest, CompletionResult, ContentPart, FinishReason, ImageGenParams,
    ImageSizeValue, MessageContent, Role, ToolCall, ToolDef,
};
