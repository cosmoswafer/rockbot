use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use tracing::debug;
use webdav::{WebDavClient, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::tool::Tool;
use crate::validated::NonEmptyString;

#[derive(Debug, Deserialize)]
struct WebDavParams {
    action: NonEmptyString,
    path: String,
    #[serde(default)]
    room_id: Option<String>,
    #[serde(default)]
    webdav_dir: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default, rename = "oldString")]
    old_string: Option<String>,
    #[serde(default, rename = "newString")]
    new_string: Option<String>,
}

pub struct WebDavTool {
    client: WebDavClient,
}

impl WebDavTool {
    pub fn new(client: WebDavClient) -> Self {
        Self { client }
    }

    fn room_path(&self, dir_key: &str, subpath: &str) -> String {
        WebDavPath::new("").room_path(dir_key, subpath)
    }

    fn room_dir(&self, dir_key: &str) -> String {
        WebDavPath::new("").room_dir(dir_key)
    }

    async fn do_read(&self, dir_key: &str, path: &str) -> Result<String> {
        let full = self.room_path(dir_key, path);
        debug!("webdav read: {}", full);

        if is_image_extension(path) {
            let bytes = self
                .client
                .read_file(&full)
                .await
                .map_err(|e| RockBotError::Provider(format!("WebDAV read failed: {e}")))?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let mime = mime_for_path(path);
            let name = path.rsplit('/').next().unwrap_or("image");
            Ok(format!("![{}](data:{};base64,{})", name, mime, b64))
        } else {
            self.client
                .read_file_to_string(&full)
                .await
                .map_err(|e| RockBotError::Provider(format!("WebDAV read failed: {e}")))
        }
    }

    async fn do_write(&self, dir_key: &str, path: &str, content: &str) -> Result<String> {
        let full = self.room_path(dir_key, path);
        debug!("webdav write: {} ({})", full, content.len());
        self.client
            .write_file_with_fallback(&full, content.as_bytes().to_vec())
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV write failed: {e}")))?;
        Ok(format!("Written {} bytes to {}", content.len(), full))
    }

    async fn do_edit(
        &self,
        dir_key: &str,
        path: &str,
        old_string: &str,
        new_string: &str,
    ) -> Result<String> {
        let full = self.room_path(dir_key, path);
        debug!("webdav edit: {}, old_len={}, new_len={}", full, old_string.len(), new_string.len());

        let content = self
            .client
            .read_file_to_string(&full)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV read for edit failed: {e}")))?;

        if content.len() > 500_000 {
            return Err(RockBotError::ToolCallParse(format!(
                "File at {} is too large for 'edit' action ({} bytes, max 500 KB). \
                 Use 'read' + 'write' instead.",
                full,
                content.len()
            )));
        }

        let count = content.matches(old_string).count();
        if count == 0 {
            return Err(RockBotError::ToolCallParse(format!(
                "oldString not found in {}. File may have been modified since last read. \
                 Use 'read' to get current content, then retry.",
                full
            )));
        }
        if count > 1 {
            return Err(RockBotError::ToolCallParse(format!(
                "oldString found {count} times in {}. \
                 Provide more surrounding context to make it unique.",
                full
            )));
        }

        let new_content = content.replace(old_string, new_string);
        self.client
            .write_file_with_fallback(&full, new_content.as_bytes().to_vec())
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV write after edit failed: {e}")))?;

        Ok(format!(
            "Edited {}: replaced 1 occurrence ({} bytes written)",
            full,
            new_content.len()
        ))
    }

    async fn do_list(&self, dir_key: &str, path: &str) -> Result<String> {
        let dir = if path.is_empty() {
            self.room_dir(dir_key)
        } else {
            self.room_path(dir_key, path)
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

    async fn do_mkdir(&self, dir_key: &str, path: &str) -> Result<String> {
        let dir = self.room_path(dir_key, path);
        debug!("webdav mkdir: {}", dir);
        self.client
            .ensure_directory_all(&dir)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV mkdir failed: {e}")))?;
        Ok(format!("Directory created: {}", dir))
    }

    async fn do_delete(&self, dir_key: &str, path: &str) -> Result<String> {
        let full = self.room_path(dir_key, path);
        debug!("webdav delete: {}", full);
        self.client
            .delete(&full)
            .await
            .map_err(|e| RockBotError::Provider(format!("WebDAV delete failed: {e}")))?;
        Ok(format!("Deleted: {}", full))
    }

    async fn do_exists(&self, dir_key: &str, path: &str) -> Result<String> {
        let full = self.room_path(dir_key, path);
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
         edit (replace oldString with newString — reads file first, fails if oldString \
         not found or matches multiple times, 500 KB max), \
         list (list directory contents), mkdir (create directory tree), \
         delete (remove file/directory), exists (check if path exists)."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "write", "edit", "list", "mkdir", "delete", "exists"],
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
                },
                "oldString": {
                    "type": "string",
                    "description": "Exact text to find and replace (required for 'edit' action, must be unique in the file)"
                },
                "newString": {
                    "type": "string",
                    "description": "Replacement text (required for 'edit' action)"
                }
            },
            "required": ["action", "path"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let params: WebDavParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse webdav arguments: {}", e))
        })?;

        let room_id = params.room_id.as_deref().unwrap_or("unknown");
        let webdav_dir = params.webdav_dir.as_deref().unwrap_or(room_id);

        match params.action.as_str() {
            "read" => self.do_read(webdav_dir, &params.path).await,
            "write" => {
                let content = params.content.as_deref().ok_or_else(|| {
                    RockBotError::ToolCallParse("webdav write requires 'content' field".into())
                })?;
                self.do_write(webdav_dir, &params.path, content).await
            }
            "edit" => {
                let old_string = params.old_string.as_deref().ok_or_else(|| {
                    RockBotError::ToolCallParse("webdav edit requires 'oldString' field".into())
                })?;
                let new_string = params.new_string.as_deref().ok_or_else(|| {
                    RockBotError::ToolCallParse("webdav edit requires 'newString' field".into())
                })?;
                self.do_edit(webdav_dir, &params.path, old_string, new_string).await
            }
            "list" => self.do_list(webdav_dir, &params.path).await,
            "mkdir" => self.do_mkdir(webdav_dir, &params.path).await,
            "delete" => self.do_delete(webdav_dir, &params.path).await,
            "exists" => self.do_exists(webdav_dir, &params.path).await,
            other => Err(RockBotError::ToolCallParse(format!(
                "Unknown webdav action: {other}. Valid: read, write, list, mkdir, delete, exists"
            ))),
        }
    }
}

fn is_image_extension(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".svg")
}

fn mime_for_path(path: &str) -> &str {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/png"
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
                .contains(&serde_json::json!("path"))
        );

        let actions = params["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), 7);
        assert!(actions.contains(&serde_json::json!("read")));
        assert!(actions.contains(&serde_json::json!("write")));
        assert!(actions.contains(&serde_json::json!("edit")));
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
    async fn test_execute_missing_room_id_uses_default() {
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

    #[tokio::test]
    async fn test_execute_edit_missing_old_string() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        let result = tool
            .execute(r#"{"action": "edit", "room_id": "general", "path": "notes.txt", "newString": "new"}"#)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("oldString"));
    }

    #[tokio::test]
    async fn test_execute_edit_missing_new_string() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        let result = tool
            .execute(r#"{"action": "edit", "room_id": "general", "path": "notes.txt", "oldString": "old"}"#)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("newString"));
    }

    #[test]
    fn test_webdav_dir_channel_path() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        assert_eq!(
            tool.room_path("r-atomkb", "notes.txt"),
            "//r-atomkb/notes.txt"
        );
    }

    #[test]
    fn test_webdav_dir_dm_path() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        assert_eq!(tool.room_path("d-saru", "data.csv"), "//d-saru/data.csv");
    }

    #[test]
    fn test_webdav_dir_channel_dir() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        assert_eq!(tool.room_dir("r-general"), "//r-general/");
    }

    #[test]
    fn test_webdav_dir_dm_dir() {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        let tool = WebDavTool::new(client);
        assert_eq!(tool.room_dir("d-alice"), "//d-alice/");
    }

    #[test]
    fn test_is_image_extension_png() {
        assert!(is_image_extension("photo.png"));
        assert!(is_image_extension("/path/to/photo.PNG"));
        assert!(is_image_extension("image.JPG"));
        assert!(is_image_extension("image.jpeg"));
        assert!(is_image_extension("anim.gif"));
        assert!(is_image_extension("icon.svg"));
        assert!(is_image_extension("photo.webp"));
    }

    #[test]
    fn test_is_image_extension_no_match() {
        assert!(!is_image_extension("notes.txt"));
        assert!(!is_image_extension("document.pdf"));
        assert!(!is_image_extension("script.py"));
        assert!(!is_image_extension("imagepng")); // no dot
    }

    #[test]
    fn test_mime_for_path_png() {
        assert_eq!(mime_for_path("photo.png"), "image/png");
        assert_eq!(mime_for_path("photo.jpg"), "image/jpeg");
        assert_eq!(mime_for_path("photo.jpeg"), "image/jpeg");
        assert_eq!(mime_for_path("anim.gif"), "image/gif");
        assert_eq!(mime_for_path("icon.svg"), "image/svg+xml");
        assert_eq!(mime_for_path("photo.webp"), "image/webp");
        assert_eq!(mime_for_path("unknown.xyz"), "image/png"); // default
    }
}
