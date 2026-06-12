use async_trait::async_trait;
use serde::Deserialize;
use tracing::debug;
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

#[derive(Debug, Deserialize)]
pub struct EditSoulParams {
    pub content: String,
    #[serde(default)]
    pub webdav_dir: Option<String>,
    #[serde(default)]
    pub room_id: Option<String>,
}

pub struct EditSoulTool {
    webdav: WebDavClient,
}

impl EditSoulTool {
    pub fn new(webdav: WebDavClient) -> Self {
        Self { webdav }
    }

    async fn do_replace(&self, dir_key: &str, content: &str) -> Result<String> {
        let path = soul_path(dir_key);
        self.webdav
            .write_file_with_fallback(&path, content.as_bytes().to_vec())
            .await
            .map_err(|e| RockBotError::Provider(format!("Soul write failed: {e}")))?;
        Ok("Soul memory updated.".to_string())
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
        "Overwrite the bot's permanent soul memory for this room. \
         The soul is a flat enumeration list — each line is a \"- \" bullet item. \
         Provide the full soul.md content using this template: \
         # Soul Memory\n\
         \n\
         - My name is YourName ✨\n\
         - (optional preference)\n\
         - (optional fact)\n\
         - (optional preference)\n\
         - (optional fact)"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Full soul.md content following the template: # Soul Memory\\n\\n- My name is Name ✨\\n- ...\\n- ...\""
                },
                "webdav_dir": {
                    "type": "string",
                    "description": "Room WebDAV directory key (injected automatically)"
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        debug!("edit_soul execute: {}", arguments);
        let params: EditSoulParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse edit_soul arguments: {e}"))
        })?;

        let webdav_dir = params
            .webdav_dir
            .as_deref()
            .or(params.room_id.as_deref())
            .unwrap_or("unknown");

        self.do_replace(webdav_dir, &params.content).await
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
        let desc = tool.description();
        assert!(desc.contains("soul memory"));
        assert!(desc.contains("flat enumeration list"));
        assert!(desc.contains("- My name is"));
        assert!(!desc.contains("## Identity"), "Description must not reference old format");

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"].get("content").is_some());
        assert!(params["properties"].get("webdav_dir").is_some());
        let required = params["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("content")));
    }

    #[tokio::test]
    async fn test_execute_missing_content() {
        let webdav = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = EditSoulTool::new(webdav);
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("content"));
    }

    #[test]
    fn test_soul_path_construction() {
        let path = soul_path("r-general");
        assert_eq!(path, "//r-general/memory/soul.md");
    }
}
