use async_trait::async_trait;
use serde::Deserialize;
use tracing::debug;

use crate::error::Result;
use crate::tool::Tool;

#[derive(Debug, Deserialize)]
pub struct ResetMemoryParams {
    #[serde(default)]
    pub room_id: Option<String>,
}

pub struct ResetMemoryTool;

impl ResetMemoryTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ResetMemoryTool {
    fn name(&self) -> &str {
        "reset_memory"
    }

    fn description(&self) -> &str {
        "Clear all conversation memory for this room instantly. \
         Use when the user says `!reset`, `!clearmemory`, or explicitly asks to clear/reset memory. \
         No LLM call or summary generation — memory is wiped immediately."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "room_id": {
                    "type": "string",
                    "description": "Room UUID (injected automatically)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        debug!("reset_memory execute: {}", arguments);
        Err(crate::error::RockBotError::ToolCallParse(
            "reset_memory must be executed via AgentHarness::process_message()".into(),
        ))
    }
}
