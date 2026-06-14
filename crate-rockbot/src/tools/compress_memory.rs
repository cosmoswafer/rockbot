use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use crate::error::Result;
use crate::tool::Tool;

/// User-explicit memory compression: compresses ALL Layer 1 messages into
/// summary.md, then clears the history entirely (zero messages remain).
#[derive(Debug, Deserialize)]
pub struct CompressMemoryParams {
    #[serde(default)]
    pub webdav_dir: Option<String>,
    #[serde(default)]
    pub room_id: Option<String>,
}

pub struct CompressMemoryTool {
    harness: Arc<Mutex<crate::AgentHarness>>,
}

impl CompressMemoryTool {
    pub fn new(harness: Arc<Mutex<crate::AgentHarness>>) -> Self {
        Self { harness }
    }
}

#[async_trait]
impl Tool for CompressMemoryTool {
    fn name(&self) -> &str {
        "compress_memory"
    }

    fn description(&self) -> &str {
        "Compress all current conversation messages into a memory summary. \
         The LLM will distill all messages into at most 10 bullet points saved as summary.md. \
         After compression, the chat history is cleared to zero — only the summary remains. \
         Use when the user says !compress, !memory, or explicitly asks to save memory."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "webdav_dir": {
                    "type": "string",
                    "description": "Room WebDAV directory key (injected automatically)"
                },
                "room_id": {
                    "type": "string",
                    "description": "Room UUID (injected automatically)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        debug!("compress_memory execute: {}", arguments);
        let params: CompressMemoryParams = serde_json::from_str(arguments).map_err(|e| {
            crate::error::RockBotError::ToolCallParse(format!("compress_memory parse error: {e}"))
        })?;

        let room_id = params.room_id.as_deref().unwrap_or("");
        if room_id.is_empty() {
            return Err(crate::error::RockBotError::ToolCallParse(
                "compress_memory requires room_id".into(),
            ));
        }

        let mut harness = self.harness.lock().await;
        harness.compress_room_full(room_id).await
    }
}
