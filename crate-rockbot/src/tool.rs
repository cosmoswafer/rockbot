use std::collections::HashMap;

use async_trait::async_trait;

use crate::error::Result;
use crate::types::ToolDef;

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub call_id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(
        call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            name: name.into(),
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(
        call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            name: name.into(),
            content: content.into(),
            is_error: true,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, arguments: &str) -> Result<String>;

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

    pub async fn execute_by_name(&self, name: &str, arguments: &str) -> Result<ToolResult> {
        match self.get(name) {
            Some(tool) => {
                let result = tool.execute(arguments).await;
                match result {
                    Ok(content) => Ok(ToolResult::success("", name, content)),
                    Err(e) => Ok(ToolResult::error(
                        "",
                        name,
                        format!("Tool execution error: {}", e),
                    )),
                }
            }
            None => Ok(ToolResult::error(
                "",
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
    fn test_tool_result_success() {
        let result = ToolResult::success("call_1", "test", "result text");
        assert_eq!(result.call_id, "call_1");
        assert_eq!(result.name, "test");
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
        let result = registry.execute_by_name("unknown", "{}").await.unwrap();
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

        let result = registry.execute_by_name("hello", "{}").await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "Hello from tool!");
    }
}
