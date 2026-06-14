use async_trait::async_trait;
use serde::Deserialize;
use tracing::debug;

use crate::error::Result;
use crate::tool::Tool;

/// User-explicit memory compression: compresses ALL Layer 1 messages into
/// summary.md, then clears the history entirely (zero messages remain).
///
/// NOTE: compress_memory is intercepted in AgentHarness::process_message()
/// and calls compress_room_full() directly on &mut self (which holds the
/// harness lock).  This tool exists only for LLM tool-registration and
/// argument injection; execute() is never reached in the main code path.
#[derive(Debug, Deserialize)]
pub struct CompressMemoryParams {
    #[serde(default)]
    pub webdav_dir: Option<String>,
    #[serde(default)]
    pub room_id: Option<String>,
}

pub struct CompressMemoryTool;

impl CompressMemoryTool {
    pub fn new() -> Self {
        Self
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
        // compress_memory is intercepted in AgentHarness::process_message(),
        // which calls compress_room_full() on &mut self directly.
        // If this is reached (e.g. tests), the caller must have already
        // obtained the harness lock, or must not be holding it.
        Err(crate::error::RockBotError::ToolCallParse(
            "compress_memory must be executed via AgentHarness::process_message()".into(),
        ))
    }
}
