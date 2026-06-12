use async_trait::async_trait;
use tracing::debug;
use webdav::WebDavClient;

use crate::error::{Result, RockBotError};
use crate::knowledge::{KnowledgeManager, SaveKnowledgeParams};
use crate::tool::Tool;

pub struct SaveKnowledgeTool {
    webdav: WebDavClient,
}

impl SaveKnowledgeTool {
    pub fn new(webdav: WebDavClient) -> Self {
        Self { webdav }
    }
}

fn split_tags(value: Option<&str>) -> Vec<String> {
    value
        .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
        .unwrap_or_default()
}

#[async_trait]
impl Tool for SaveKnowledgeTool {
    fn name(&self) -> &str {
        "save_knowledge"
    }

    fn description(&self) -> &str {
        "Save a piece of knowledge (skill, secret, or note) for future reference. \
         Use this when the user says 'remember', 'learn', or shares important \
         information worth persisting. Each entry gets a .md file and is indexed \
         for contextual retrieval."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": ["skill", "secret", "note"],
                    "description": "Knowledge category: skill (procedural/how-to), \
                                    secret (credential/sensitive), note (factual info)"
                },
                "topic": {
                    "type": "string",
                    "description": "Short title or topic for the entry (e.g. 'DB API', 'Build Commands')"
                },
                "content": {
                    "type": "string",
                    "description": "Markdown body of the knowledge entry"
                },
                "when_useful": {
                    "type": "string",
                    "description": "Describe the situation that makes this knowledge relevant, \
                                    used for automatic retrieval (e.g. 'when calling the database API')"
                },
                "tags": {
                    "type": "string",
                    "description": "Comma-separated keywords for search (e.g. 'api, database, python')"
                },
                "priority": {
                    "type": "string",
                    "enum": ["P0", "P1", "P2", "P3"],
                    "description": "Knowledge priority: P0 (highest, always recalled), P1 (high, default), P2 (medium), P3 (low). Higher priority means more frequently recalled."
                }
            },
            "required": ["category", "topic", "content", "when_useful"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        debug!("save_knowledge execute: {}", arguments);
        let params: SaveKnowledgeParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse save_knowledge arguments: {e}"))
        })?;

        let tags = split_tags(params.tags.as_deref());
        let webdav_dir = params.webdav_dir.as_deref().unwrap_or("unknown");

        KnowledgeManager::save_entry(
            &self.webdav,
            webdav_dir,
            &params.category,
            &params.topic,
            &params.content,
            &params.when_useful,
            &tags,
            &params.priority,
        )
        .await?;

        Ok(format!(
            "Knowledge saved: [{}/{}] {}",
            params.category, params.topic, params.topic
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_knowledge_tool_definition() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = SaveKnowledgeTool::new(webdav);
        assert_eq!(tool.name(), "save_knowledge");
        assert!(tool.description().contains("Save a piece of knowledge"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        let cats = params["properties"]["category"]["enum"].as_array().unwrap();
        assert!(cats.contains(&serde_json::json!("skill")));
        assert!(cats.contains(&serde_json::json!("secret")));
        assert!(cats.contains(&serde_json::json!("note")));
    }

    #[tokio::test]
    async fn test_execute_missing_category() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = SaveKnowledgeTool::new(webdav);
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_category() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = SaveKnowledgeTool::new(webdav);
        let result = tool
            .execute(r#"{"category": "invalid", "topic": "test", "content": "x", "when_useful": "always", "webdav_dir": "r-test"}"#)
            .await;
        assert!(result.is_err(), "Expected error for invalid category, got: {result:?}");
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("category") || err_str.contains("invalid"), "Unexpected error: {err_str}");
    }

    #[test]
    fn test_split_tags() {
        let tags = split_tags(Some("api, database, rust"));
        assert_eq!(tags, vec!["api", "database", "rust"]);

        let tags = split_tags(Some(""));
        assert!(tags.is_empty());

        let tags = split_tags(None);
        assert!(tags.is_empty());
    }
}
