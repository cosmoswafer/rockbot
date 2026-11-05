use async_trait::async_trait;
use serde::Deserialize;
use tracing::debug;
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::memory::{SoulDocument, SoulName};
use crate::tool::Tool;
use crate::validated::NonEmptyString;

#[derive(Debug, Deserialize)]
pub struct EditSoulParams {
    pub content: NonEmptyString,
    #[serde(default)]
    pub webdav_dir: Option<String>,
    #[serde(default)]
    pub room_id: Option<String>,
    /// Injected by the harness — last known identity name for this room.
    #[serde(default)]
    pub previous_name: Option<String>,
}

pub struct EditSoulTool {
    webdav: WebDavClient,
}

impl EditSoulTool {
    pub fn new(webdav: WebDavClient) -> Self {
        Self { webdav }
    }

    async fn do_replace(
        &self,
        dir_key: &str,
        raw: &str,
        previous_name: Option<&SoulName>,
    ) -> Result<String> {
        let doc = SoulDocument::parse(raw);
        if let Some(reason) = &doc.broken_front_matter {
            // Write path is stricter than the read path: refuse to persist a
            // mangled block instead of silently dropping it, so the LLM retries.
            return Err(RockBotError::SoulFormat(format!(
                "soul.md front matter is not valid flat YAML ({reason}); \
                 rewrite the `---` block as `key: value` lines"
            )));
        }
        let doc = doc.normalized_for_write(previous_name);
        let rendered = doc.render();
        let path = soul_path(dir_key);
        self.webdav
            .write_file_with_fallback(&path, rendered.clone().into_bytes())
            .await
            .map_err(|e| RockBotError::Provider(format!("Soul write failed: {e}")))?;
        debug!(
            "edit_soul wrote {} chars (front_matter={}, name={:?})",
            rendered.chars().count(),
            doc.front.name.is_some(),
            doc.front.name.as_ref().map(SoulName::as_str),
        );
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
         Provide the full soul.md content: an optional YAML front matter block with identity metadata, \
         then a flat \"- \" bullet list of preferences and facts. Template:\n\
         ---\n\
         name: YourName ✨\n\
         address_user_as: Master 🐵\n\
         language: en\n\
         ---\n\
         \n\
         # Soul Memory\n\
         \n\
         - (optional preference)\n\
         - (optional fact)\n\
         `name` is a single line, max 32 characters, no markdown. Front matter is OPTIONAL — \
         omit it entirely for a plain list. Keep the body a flat list. \
         Do not add `schema` or `updated` — the bot stamps them automatically."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Full soul.md content following the template: optional `---` front matter with name/address_user_as/language, then `# Soul Memory` and a flat `- ` bullet list."
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

        let previous_name = params
            .previous_name
            .as_deref()
            .and_then(|value| SoulName::try_new(value).ok());

        self.do_replace(webdav_dir, &params.content, previous_name.as_ref()).await
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
        assert!(desc.contains("name: YourName ✨"));
        assert!(desc.contains("---"));
        assert!(desc.contains("Front matter is OPTIONAL"));
        assert!(desc.contains("Do not add `schema` or `updated`"));
        assert!(!desc.contains("## Identity"), "Description must not reference old format");

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"].get("content").is_some());
        assert!(params["properties"].get("webdav_dir").is_some());
        assert!(
            params["properties"].get("previous_name").is_none(),
            "previous_name is harness-injected, not LLM-facing"
        );
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
