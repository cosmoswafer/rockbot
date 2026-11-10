use std::collections::HashMap;

use async_trait::async_trait;

use crate::error::Result;
use crate::types::ToolDef;
use crate::validated::NonEmptyString;

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub call_id: NonEmptyString,
    pub name: NonEmptyString,
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(
        call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let call_id = NonEmptyString::try_new(call_id.into())
            .expect("ToolResult::success call_id must be non-empty");
        let name = NonEmptyString::try_new(name.into())
            .expect("ToolResult::success name must be non-empty");
        Self {
            call_id,
            name,
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(
        call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let call_id = NonEmptyString::try_new(call_id.into())
            .unwrap_or_else(|_| NonEmptyString::try_new("unknown".to_string()).unwrap());
        let name = NonEmptyString::try_new(name.into())
            .unwrap_or_else(|_| NonEmptyString::try_new("unknown".to_string()).unwrap());
        Self {
            call_id,
            name,
            content: content.into(),
            is_error: true,
        }
    }
}

/// Default budget for the arguments one tool call may carry from the model.
/// Larger payloads must go through a chunked or file-backed route.
pub const DEFAULT_MAX_TOOL_ARG_BYTES: usize = 8 * 1024;

/// Budget for the designated large-content tools (`webdav`, `webdav_edit`):
/// whole-file writes and patch blocks legitimately exceed the default.
pub const LARGE_CONTENT_MAX_ARG_BYTES: usize = 64 * 1024;

/// Budget for `save_knowledge`, whose markdown body can be several KiB.
pub const KNOWLEDGE_MAX_ARG_BYTES: usize = 32 * 1024;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, arguments: &str) -> Result<String>;

    /// Budget for the arguments this tool accepts from the model. The harness
    /// rejects larger arguments before execution: a call this big is usually a
    /// serialization truncated by the output budget (issue #110).
    fn max_arg_bytes(&self) -> usize {
        DEFAULT_MAX_TOOL_ARG_BYTES
    }

    fn to_def(&self) -> ToolDef {
        ToolDef::new(self.name(), self.description(), self.parameters())
    }
}

pub type ToolMap = HashMap<String, Box<dyn Tool>>;

#[derive(Default)]
pub struct ToolRegistry {
    tools: ToolMap,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn definitions(&self) -> Vec<ToolDef> {
        self.tools.values().map(|t| t.to_def()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub async fn execute_by_name(
        &self,
        call_id: &str,
        name: &str,
        arguments: &str,
    ) -> Result<ToolResult> {
        match self.get(name) {
            Some(tool) => {
                let result = tool.execute(arguments).await;
                match result {
                    Ok(content) => Ok(ToolResult::success(call_id, name, content)),
                    Err(e) => Ok(ToolResult::error(
                        call_id,
                        name,
                        format!("Tool execution error: {}", e),
                    )),
                }
            }
            None => Ok(ToolResult::error(
                call_id,
                name,
                format!("Unknown tool: {}", name),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTool {
        name: String,
        desc: String,
        params: serde_json::Value,
        result: String,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.desc
        }

        fn parameters(&self) -> serde_json::Value {
            self.params.clone()
        }

        async fn execute(&self, _arguments: &str) -> Result<String> {
            Ok(self.result.clone())
        }
    }

    #[test]
    fn test_tool_default_arg_budget() {
        let tool = MockTool {
            name: "test_tool".into(),
            desc: "A test tool".into(),
            params: serde_json::json!({}),
            result: "ok".into(),
        };
        assert_eq!(tool.max_arg_bytes(), DEFAULT_MAX_TOOL_ARG_BYTES);
        assert_eq!(DEFAULT_MAX_TOOL_ARG_BYTES, 8 * 1024);
    }

    struct LargeArgMockTool;

    #[async_trait]
    impl Tool for LargeArgMockTool {
        fn name(&self) -> &str {
            "large"
        }

        fn description(&self) -> &str {
            "Accepts large payloads"
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "required": []})
        }

        fn max_arg_bytes(&self) -> usize {
            LARGE_CONTENT_MAX_ARG_BYTES
        }

        async fn execute(&self, _arguments: &str) -> Result<String> {
            Ok("ok".into())
        }
    }

    #[test]
    fn test_tool_can_override_arg_budget() {
        assert_eq!(LargeArgMockTool.max_arg_bytes(), 64 * 1024);
    }

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("call_1", "test", "result text");
        assert_eq!(result.call_id.as_str(), "call_1");
        assert_eq!(result.name.as_str(), "test");
        assert_eq!(result.content, "result text");
        assert!(!result.is_error);
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("call_1", "test", "failed");
        assert!(result.is_error);
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = ToolRegistry::new();
        let tool = Box::new(MockTool {
            name: "test_tool".into(),
            desc: "A test tool".into(),
            params: serde_json::json!({}),
            result: "ok".into(),
        });
        registry.register(tool);

        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
        assert!(registry.get("test_tool").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_definitions() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(MockTool {
            name: "tool_a".into(),
            desc: "Tool A".into(),
            params: serde_json::json!({"type": "object"}),
            result: "a".into(),
        }));
        registry.register(Box::new(MockTool {
            name: "tool_b".into(),
            desc: "Tool B".into(),
            params: serde_json::json!({"type": "object"}),
            result: "b".into(),
        }));

        let defs = registry.definitions();
        assert_eq!(defs.len(), 2);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[tokio::test]
    async fn test_execute_unknown_tool() {
        let registry = ToolRegistry::new();
        let result = registry.execute_by_name("", "unknown", "{}").await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_execute_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(MockTool {
            name: "hello".into(),
            desc: "Says hello".into(),
            params: serde_json::json!({}),
            result: "Hello from tool!".into(),
        }));

        let result = registry.execute_by_name("call_1", "hello", "{}").await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "Hello from tool!");
    }
}
