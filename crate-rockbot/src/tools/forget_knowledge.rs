use async_trait::async_trait;
use tracing::debug;
use webdav::WebDavClient;

use crate::error::{Result, RockBotError};
use crate::knowledge::{ForgetKnowledgeParams, KnowledgeManager};
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
        let params: ForgetKnowledgeParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse forget_knowledge arguments: {e}"))
        })?;

        let webdav_dir = params.webdav_dir.as_deref().unwrap_or("unknown");

        KnowledgeManager::delete_entry(&self.webdav, webdav_dir, &params.topic).await?;

        Ok(format!("Knowledge entry '{}' deleted.", params.topic))
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

    #[test]
    fn test_forget_knowledge_tool_description() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ForgetKnowledgeTool::new(webdav);
        let desc = tool.description();
        assert!(desc.to_lowercase().contains("delete"), "description should contain 'delete'");
        assert!(desc.contains("index"), "description should contain 'index'");
    }

    #[tokio::test]
    async fn test_execute_missing_topic() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = ForgetKnowledgeTool::new(webdav);
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }
}
