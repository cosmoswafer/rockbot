pub mod deepseek;
pub mod openrouter;
pub mod replicate;

use async_trait::async_trait;

use crate::types::{ChatRequest, CompletionResult};

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, request: ChatRequest) -> crate::error::Result<CompletionResult>;

    fn provider_name(&self) -> &str;

    fn model_name(&self) -> &str;
}

pub use deepseek::DeepSeekProvider;
pub use openrouter::OpenRouterProvider;
pub use replicate::ReplicateProvider;
