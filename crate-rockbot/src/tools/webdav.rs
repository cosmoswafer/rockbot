use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

pub struct WebDavTool {
    client: WebDavClient,
}

impl WebDavTool {
    pub fn new(client: WebDavClient) -> Self {
        Self { client }
    }

    fn room_path(&self, room_id: &str, subpath: &str) -> String {
        WebDavPath::new("").room_path(room_id, subpath)
    }

    fn room_dir(&self, room_id: &str) -> String {
        WebDavPath::new("").room_dir(room_id)
    }

    async fn do_read(&self, room_id: &str, path: &str) -> Result<String> {
        let full = self.room_path(room_id, path);
        debug!("webdav read: {}", full);
        self.client
            .read_file_to_string(&full)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV read failed: {e}")))
    }

    async fn do_write(&self, room_id: &str, path: &str, content: &str) -> Result<String> {
        let full = self.room_path(room_id, path);
        debug!("webdav write: {} ({})", full, content.len());
        self.client
            .write_file_with_fallback(&full, content.as_bytes().to_vec())
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV write failed: {e}")))?;
        Ok(format!("Written {} bytes to {}", content.len(), full))
    }

    async fn do_list(&self, room_id: &str, path: &str) -> Result<String> {
        let dir = if path.is_empty() {
            self.room_dir(room_id)
        } else {
            self.room_path(room_id, path)
        };
        debug!("webdav list: {}", dir);
        let entries = self
            .client
            .list_directory(&dir)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV list failed: {e}")))?;

        if entries.is_empty() {
            return Ok(format!("Directory '{}' is empty.", dir));
        }

        let mut out = format!("Contents of '{}':\n\n", dir);
        for entry in &entries {
            let kind = if entry.is_dir { "DIR " } else { "FILE" };
            out.push_str(&format!(
                "  {kind}  {:>10}  {}  {}\n",
                format_size(entry.size),
                entry.modified,
                entry.name
            ));
        }
        Ok(out)
    }

    async fn do_mkdir(&self, room_id: &str, path: &str) -> Result<String> {
        let dir = self.room_path(room_id, path);
        debug!("webdav mkdir: {}", dir);
        self.client
            .ensure_directory_all(&dir)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV mkdir failed: {e}")))?;
        Ok(format!("Directory created: {}", dir))
    }

    async fn do_delete(&self, room_id: &str, path: &str) -> Result<String> {
        let full = self.room_path(room_id, path);
        debug!("webdav delete: {}", full);
        self.client
            .delete(&full)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV delete failed: {e}")))?;
        Ok(format!("Deleted: {}", full))
    }

    async fn do_exists(&self, room_id: &str, path: &str) -> Result<String> {
        let full = self.room_path(room_id, path);
        debug!("webdav exists: {}", full);
        let exists = self
            .client
            .exists(&full)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV exists check failed: {e}")))?;
        Ok(format!(
            "Path '{}': {}",
            full,
            if exists { "exists" } else { "not found" }
        ))
    }
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "-".to_string();
    }
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[async_trait]
impl Tool for WebDavTool {
    fn name(&self) -> &str {
        "webdav"
    }

    fn description(&self) -> &str {
        "Manage files on remote WebDAV storage (NextCloud). \
         Each room has its own file space — paths are automatically scoped. \
         Actions: read (get file content), write (create/overwrite a file), \
         list (list directory contents), mkdir (create directory tree), \
         delete (remove file/directory), exists (check if path exists)."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "write", "list", "mkdir", "delete", "exists"],
                    "description": "The WebDAV operation to perform"
                },
                "room_id": {
                    "type": "string",
                    "description": "Room ID for scoping the operation (injected automatically if omitted)"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory path relative to the room root"
                },
                "content": {
                    "type": "string",
                    "description": "File content to write (required for 'write' action)"
                }
            },
            "required": ["action", "room_id", "path"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse webdav arguments: {e}"))
        })?;

        let action = args
            .get("action")
            .and_then(|a| a.as_str())
            .ok_or_else(|| RockBotError::ToolCallParse("webdav requires 'action' field".into()))?;

        let room_id = args
            .get("room_id")
            .and_then(|r| r.as_str())
            .ok_or_else(|| RockBotError::ToolCallParse("webdav requires 'room_id' field".into()))?;

        let path = args
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| RockBotError::ToolCallParse("webdav requires 'path' field".into()))?;

        match action {
            "read" => self.do_read(room_id, path).await,
            "write" => {
                let content = args
                    .get("content")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| {
                        RockBotError::ToolCallParse("webdav write requires 'content' field".into())
                    })?;
                self.do_write(room_id, path, content).await
            }
            "list" => self.do_list(room_id, path).await,
            "mkdir" => self.do_mkdir(room_id, path).await,
            "delete" => self.do_delete(room_id, path).await,
            "exists" => self.do_exists(room_id, path).await,
            other => Err(RockBotError::ToolCallParse(format!(
                "Unknown webdav action: {other}. Valid: read, write, list, mkdir, delete, exists"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webdav_tool_definition() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        assert_eq!(tool.name(), "webdav");
        assert!(tool.description().contains("WebDAV"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("action"))
        );
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("room_id"))
        );
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("path"))
        );

        let actions = params["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), 6);
        assert!(actions.contains(&serde_json::json!("read")));
        assert!(actions.contains(&serde_json::json!("write")));
        assert!(actions.contains(&serde_json::json!("list")));
        assert!(actions.contains(&serde_json::json!("mkdir")));
        assert!(actions.contains(&serde_json::json!("delete")));
        assert!(actions.contains(&serde_json::json!("exists")));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "-");
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1572864), "1.5 MB");
    }

    #[test]
    fn test_room_path_construction() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        assert_eq!(
            tool.room_path("general", "notes.txt"),
            "//general/notes.txt"
        );
        assert_eq!(
            tool.room_path("general", "/workspace/readme.md"),
            "//general/workspace/readme.md"
        );
    }

    #[test]
    fn test_room_dir_construction() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        assert_eq!(tool.room_dir("general"), "//general/");
    }

    #[tokio::test]
    async fn test_execute_missing_action() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_unknown_action() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        let result = tool
            .execute(r#"{"action": "unknown", "room_id": "x", "path": "x"}"#)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown webdav action")
        );
    }

    #[tokio::test]
    async fn test_execute_missing_room_id() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        let result = tool.execute(r#"{"action": "list", "path": "/"}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_missing_path() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        let result = tool
            .execute(r#"{"action": "read", "room_id": "general"}"#)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_write_missing_content() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        let result = tool
            .execute(r#"{"action": "write", "room_id": "general", "path": "notes.txt"}"#)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        let result = tool.execute("not json").await;
        assert!(result.is_err());
    }
}
