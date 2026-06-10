use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;
use webdav::WebDavClient;

use crate::error::{Result, RockBotError};
use crate::knowledge::KnowledgeManager;
use crate::tool::Tool;

pub struct ForgetKnowledgeTool {
    webdav: WebDavClient,
}

impl ForgetKnowledgeTool {
    pub fn new(webdav: WebDavClient) -> Self {
        Self { webdav }
    }
}

#[async_trait]
impl Tool for ForgetKnowledgeTool {
    fn name(&self) -> &str {
        "forget_knowledge"
    }

    fn description(&self) -> &str {
        "Remove a previously saved knowledge entry. Provide the topic title \
         of the entry to delete. The .md file is deleted and the entry is \
         removed from the knowledge index."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Title or topic of the knowledge entry to delete"
                }
            },
            "required": ["topic"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        debug!("forget_knowledge execute: {}", arguments);
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse forget_knowledge arguments: {e}"))
        })?;

        let topic = args.get("topic").and_then(|t| t.as_str()).ok_or_else(|| {
            RockBotError::ToolCallParse("forget_knowledge requires 'topic' field".into())
        })?;

        let webdav_dir = args
            .get("webdav_dir")
            .and_then(|d| d.as_str())
            .unwrap_or("unknown");

        KnowledgeManager::delete_entry(&self.webdav, webdav_dir, topic).await?;

        Ok(format!("Knowledge entry '{}' deleted.", topic))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forget_knowledge_tool_definition() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ForgetKnowledgeTool::new(webdav);
        assert_eq!(tool.name(), "forget_knowledge");
        assert!(tool.description().contains("Remove a previously saved knowledge"));
    }

    #[tokio::test]
    async fn test_execute_missing_topic() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ForgetKnowledgeTool::new(webdav);
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }
}
