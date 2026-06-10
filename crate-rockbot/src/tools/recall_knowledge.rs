use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;
use webdav::WebDavClient;

use crate::error::{Result, RockBotError};
use crate::knowledge::KnowledgeManager;
use crate::tool::Tool;

pub struct RecallKnowledgeTool {
    webdav: WebDavClient,
}

impl RecallKnowledgeTool {
    pub fn new(webdav: WebDavClient) -> Self {
        Self { webdav }
    }
}

#[async_trait]
impl Tool for RecallKnowledgeTool {
    fn name(&self) -> &str {
        "recall_knowledge"
    }

    fn description(&self) -> &str {
        "Search the knowledge index for entries matching a query. \
         If no query is given, returns all stored knowledge entries. \
         Matches by topic title, when_useful description, and tags."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Topic or keyword to search for in knowledge entries. \
                                    Leave empty to retrieve all entries."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        debug!("recall_knowledge execute: {}", arguments);
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse recall_knowledge arguments: {e}"))
        })?;

        let query = args
            .get("query")
            .and_then(|q| q.as_str())
            .unwrap_or("");

        let webdav_dir = args
            .get("webdav_dir")
            .and_then(|d| d.as_str())
            .unwrap_or("unknown");

        Ok(KnowledgeManager::recall_entry(&self.webdav, webdav_dir, query)
            .await?
            .unwrap_or_else(|| "No knowledge entries found for this room.".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recall_knowledge_tool_definition() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = RecallKnowledgeTool::new(webdav);
        assert_eq!(tool.name(), "recall_knowledge");
        assert!(tool.description().contains("Search the knowledge index"));
    }
}
