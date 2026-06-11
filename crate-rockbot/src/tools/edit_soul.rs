use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

pub struct EditSoulTool {
    webdav: WebDavClient,
}

impl EditSoulTool {
    pub fn new(webdav: WebDavClient) -> Self {
        Self { webdav }
    }

    async fn do_append(&self, dir_key: &str, section_header: &str, content: &str) -> Result<String> {
        let path = soul_path(dir_key);
        let existing = self.webdav.read_file_to_string(&path).await.unwrap_or_default();

        // Check for duplicate section header to prevent double sections
        fn section_exists(text: &str, header: &str) -> bool {
            text.contains(&format!("\n## {} ", header))
                || text.contains(&format!("\n## {}", header))
                || text.starts_with(&format!("## {} ", header))
                || text.starts_with(&format!("## {}", header))
        }

        if !existing.is_empty() && section_exists(&existing, section_header) {
            return Err(RockBotError::ToolCallParse(format!(
                "Section '## {}' already exists in soul memory. Use 'replace' action to update it.",
                section_header
            )));
        }

        let new_content = if existing.is_empty() {
            format!("# Soul Memory\n\n## {}\n{}\n", section_header, content)
        } else {
            format!("{}\n## {}\n{}", existing.trim_end(), section_header, content)
        };

        self.webdav
            .write_file_with_fallback(&path, new_content.as_bytes().to_vec())
            .await
            .map_err(|e| RockBotError::Provider(format!("Soul write failed: {e}")))?;

        Ok(format!(
            "Appended section '## {}' to soul memory.",
            section_header
        ))
    }

    async fn do_replace(&self, dir_key: &str, section_header: &str, content: &str) -> Result<String> {
        let path = soul_path(dir_key);
        let existing = self.webdav.read_file_to_string(&path).await.unwrap_or_default();

        fn find_section(text: &str, header: &str) -> Option<(usize, usize)> {
            if let Some(pos) = text.find(&format!("\n## {} ", header)) {
                Some((pos, format!("\n## {} ", header).len()))
            } else if let Some(pos) = text.find(&format!("\n## {}", header)) {
                Some((pos, format!("\n## {}", header).len()))
            } else if text.starts_with(&format!("## {} ", header)) {
                Some((0, format!("## {} ", header).len()))
            } else if text.starts_with(&format!("## {}", header)) {
                Some((0, format!("## {}", header).len()))
            } else {
                None
            }
        }

        let (section_start, marker_len) = find_section(&existing, section_header).ok_or_else(|| {
            RockBotError::ToolCallParse(format!(
                "Section '## {}' not found in soul memory.",
                section_header
            ))
        })?;

        let before = &existing[..section_start];
        let rest = &existing[section_start + marker_len..];

        let next_header = rest.find("\n## ").unwrap_or(rest.len());
        let new_content = format!(
            "{}\n## {} {}\n{}",
            before,
            section_header,
            content,
            &rest[next_header..]
        );

        self.webdav
            .write_file_with_fallback(&path, new_content.as_bytes().to_vec())
            .await
            .map_err(|e| RockBotError::Provider(format!("Soul write failed: {e}")))?;

        Ok(format!(
            "Replaced section '## {}' in soul memory.",
            section_header
        ))
    }

    async fn do_delete_section(&self, dir_key: &str, section_header: &str) -> Result<String> {
        let path = soul_path(dir_key);
        let existing = self.webdav.read_file_to_string(&path).await.unwrap_or_default();

        fn find_section(text: &str, header: &str) -> Option<(usize, usize)> {
            if let Some(pos) = text.find(&format!("\n## {} ", header)) {
                Some((pos, format!("\n## {} ", header).len()))
            } else if let Some(pos) = text.find(&format!("\n## {}", header)) {
                Some((pos, format!("\n## {}", header).len()))
            } else if text.starts_with(&format!("## {} ", header)) {
                Some((0, format!("## {} ", header).len()))
            } else if text.starts_with(&format!("## {}", header)) {
                Some((0, format!("## {}", header).len()))
            } else {
                None
            }
        }

        let (section_start, marker_len) = find_section(&existing, section_header).ok_or_else(|| {
            RockBotError::ToolCallParse(format!(
                "Section '## {}' not found in soul memory.",
                section_header
            ))
        })?;

        let rest = &existing[section_start + marker_len..];
        let next_header = rest.find("\n## ").unwrap_or(rest.len());
        let end_pos = section_start + marker_len + next_header;

        let new_content = format!(
            "{}{}",
            &existing[..section_start],
            &existing[end_pos..]
        );

        self.webdav
            .write_file_with_fallback(&path, new_content.as_bytes().to_vec())
            .await
            .map_err(|e| RockBotError::Provider(format!("Soul write failed: {e}")))?;

        Ok(format!(
            "Deleted section '## {}' from soul memory.",
            section_header
        ))
    }
}

fn soul_path(dir_key: &str) -> String {
    format!("{}memory/soul.md", WebDavPath::new("").room_dir(dir_key))
}

#[async_trait]
impl Tool for EditSoulTool {
    fn name(&self) -> &str {
        "edit_soul"
    }

    fn description(&self) -> &str {
        "Edit the bot's permanent memory (soul) for this room. \
         Actions: append (add a new section or content), \
         replace (replace an existing section's content), \
         delete_section (remove a section entirely). \
         Use section_header to target the ## Section name."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["append", "replace", "delete_section"],
                    "description": "Soul memory operation: append (add new section/content), \
                                    replace (update existing section), \
                                    delete_section (remove a section)"
                },
                "section_header": {
                    "type": "string",
                    "description": "Target ## Section name (e.g. Preferences, Identity, Facts)"
                },
                "content": {
                    "type": "string",
                    "description": "New content for the section (required for append and replace)"
                },
                "webdav_dir": {
                    "type": "string",
                    "description": "Room WebDAV directory key (injected automatically)"
                }
            },
            "required": ["action", "section_header"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        debug!("edit_soul execute: {}", arguments);
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse edit_soul arguments: {e}"))
        })?;

        let action = args
            .get("action")
            .and_then(|a| a.as_str())
            .ok_or_else(|| RockBotError::ToolCallParse("edit_soul requires 'action' field".into()))?;

        let webdav_dir = args
            .get("webdav_dir")
            .and_then(|d| d.as_str())
            .or_else(|| args.get("room_id").and_then(|r| r.as_str()))
            .unwrap_or("unknown");

        let section_header = args
            .get("section_header")
            .and_then(|s| s.as_str())
            .ok_or_else(|| {
                RockBotError::ToolCallParse("edit_soul requires 'section_header' field".into())
            })?;

        match action {
            "append" => {
                let content = args.get("content").and_then(|c| c.as_str()).ok_or_else(|| {
                    RockBotError::ToolCallParse("edit_soul append requires 'content' field".into())
                })?;
                self.do_append(webdav_dir, section_header, content).await
            }
            "replace" => {
                let content = args.get("content").and_then(|c| c.as_str()).ok_or_else(|| {
                    RockBotError::ToolCallParse("edit_soul replace requires 'content' field".into())
                })?;
                self.do_replace(webdav_dir, section_header, content).await
            }
            "delete_section" => {
                self.do_delete_section(webdav_dir, section_header).await
            }
            other => Err(RockBotError::ToolCallParse(format!(
                "Unknown edit_soul action: {other}. Valid: append, replace, delete_section"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_soul_tool_definition() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = EditSoulTool::new(webdav);
        assert_eq!(tool.name(), "edit_soul");
        assert!(tool.description().contains("permanent memory"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        let actions = params["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), 3);
        assert!(actions.contains(&serde_json::json!("append")));
        assert!(actions.contains(&serde_json::json!("replace")));
        assert!(actions.contains(&serde_json::json!("delete_section")));
    }

    #[tokio::test]
    async fn test_execute_missing_action() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = EditSoulTool::new(webdav);
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_unknown_action() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = EditSoulTool::new(webdav);
        let result = tool
            .execute(r#"{"action": "unknown", "section_header": "Notes", "webdav_dir": "r-test"}"#)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown edit_soul action"));
    }

    #[tokio::test]
    async fn test_execute_append_missing_content() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = EditSoulTool::new(webdav);
        let result = tool
            .execute(r#"{"action": "append", "section_header": "Notes", "webdav_dir": "r-test"}"#)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("content"));
    }

    #[test]
    fn test_soul_path_construction() {
        let path = soul_path("r-general");
        assert_eq!(path, "//r-general/memory/soul.md");
    }
}
