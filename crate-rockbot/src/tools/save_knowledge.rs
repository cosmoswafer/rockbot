use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;
use webdav::WebDavClient;

use crate::error::{Result, RockBotError};
use crate::knowledge::{KnowledgeCategory, KnowledgeManager, KnowledgePriority};
use crate::tool::Tool;

pub struct SaveKnowledgeTool {
    webdav: WebDavClient,
}

impl SaveKnowledgeTool {
    pub fn new(webdav: WebDavClient) -> Self {
        Self { webdav }
    }
}

fn parse_tags(value: &Value) -> Vec<String> {
    value
        .get("tags")
        .and_then(|t| t.as_str())
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
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
                    "description": "Knowledge priority: P0 (highest, always recalled), P1 (high), P2 (medium, default), P3 (low). Higher priority means more frequently recalled."
                }
            },
            "required": ["category", "topic", "content", "when_useful"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        debug!("save_knowledge execute: {}", arguments);
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse save_knowledge arguments: {e}"))
        })?;

        let category_str = args.get("category").and_then(|c| c.as_str()).ok_or_else(|| {
            RockBotError::ToolCallParse("save_knowledge requires 'category' field".into())
        })?;

        let category = match category_str {
            "skill" => KnowledgeCategory::Skill,
            "secret" => KnowledgeCategory::Secret,
            "note" => KnowledgeCategory::Note,
            other => {
                return Err(RockBotError::ToolCallParse(format!(
                    "Invalid category: {other}. Valid: skill, secret, note"
                )))
            }
        };

        let topic = args.get("topic").and_then(|t| t.as_str()).ok_or_else(|| {
            RockBotError::ToolCallParse("save_knowledge requires 'topic' field".into())
        })?;

        let content = args
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                RockBotError::ToolCallParse("save_knowledge requires 'content' field".into())
            })?;

        let when_useful = args
            .get("when_useful")
            .and_then(|w| w.as_str())
            .ok_or_else(|| {
                RockBotError::ToolCallParse("save_knowledge requires 'when_useful' field".into())
            })?;

        let tags = parse_tags(&args);

        let priority = args
            .get("priority")
            .and_then(|p| p.as_str())
            .map(|s| match s {
                "P0" => KnowledgePriority::P0,
                "P1" => KnowledgePriority::P1,
                "P2" => KnowledgePriority::P2,
                "P3" => KnowledgePriority::P3,
                _ => KnowledgePriority::default(),
            })
            .unwrap_or_default();

        let webdav_dir = args
            .get("webdav_dir")
            .and_then(|d| d.as_str())
            .unwrap_or("unknown");

        KnowledgeManager::save_entry(
            &self.webdav,
            webdav_dir,
            &category,
            topic,
            content,
            when_useful,
            &tags,
            &priority,
        )
        .await?;

        Ok(format!(
            "Knowledge saved: [{}/{}] {}",
            category, topic, topic
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
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid category"));
    }

    #[test]
    fn test_parse_tags() {
        let args = serde_json::json!({"tags": "api, database, rust"});
        let tags = parse_tags(&args);
        assert_eq!(tags, vec!["api", "database", "rust"]);

        let args = serde_json::json!({"tags": ""});
        let tags = parse_tags(&args);
        assert!(tags.is_empty());

        let args = serde_json::json!({});
        let tags = parse_tags(&args);
        assert!(tags.is_empty());
    }
}
