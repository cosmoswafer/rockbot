pub mod deepseek;
pub mod fal;
pub mod openrouter;

use async_trait::async_trait;

use crate::error::Result;
use crate::types::{ChatRequest, CompletionResult, ImageGenParams};

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, request: ChatRequest) -> Result<CompletionResult>;

    fn provider_name(&self) -> &str;

    fn model_name(&self) -> &str;
}

#[async_trait]
pub trait ImageProvider: Send + Sync {
    async fn generate_image(&self, params: &ImageGenParams) -> Result<Vec<u8>>;

    async fn upload_file(&self, data: &[u8], content_type: &str) -> Result<String>;

    fn provider_name(&self) -> &str;

    fn model_id(&self) -> &str;
}

pub use deepseek::DeepSeekProvider;
pub use fal::FalAiProvider;
pub use openrouter::{OpenRouterImageProvider, OpenRouterProvider};
