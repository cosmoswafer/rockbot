//! `acp_delegate` tool — lets the LLM delegate a task to the external ACP
//! agent via [`AcpClient`]. See `_dfd/tools/acp-delegate.md`.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_valid::Validate;

use crate::acp::AcpClient;
use crate::error::{Result, RockBotError};
use crate::tool::Tool;
use crate::validated::NonEmptyString;

/// LLM-facing tool arguments — DFD: `AcpDelegateParams`.
#[derive(Debug, Deserialize, Validate)]
struct AcpDelegateParams {
    prompt: NonEmptyString,
    /// Optional per-call timeout override in seconds (10..=3600). Range is
    /// checked manually below — serde_valid does not apply numeric bounds
    /// inside `Option`.
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// The `acp_delegate` tool. Holds the single shared `AcpClient`; prompts are
/// serialized inside the client (one ACP prompt turn at a time).
pub struct AcpTool {
    client: Arc<AcpClient>,
}

impl AcpTool {
    pub fn new(client: Arc<AcpClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for AcpTool {
    fn name(&self) -> &str {
        "acp_delegate"
    }

    fn description(&self) -> &str {
        "Delegate a task to an external ACP coding agent (a separate subprocess with its own \
         tools and workspace) and return its final text output. Use for long-running or \
         coding-oriented tasks the external agent can execute autonomously (writing/editing \
         files in its workspace, running commands, multi-step work). The agent runs \
         unattended — permissions are denied unless the operator enabled auto-approve. \
         Do NOT include user secrets, tokens, or credentials in the prompt. Output is \
         truncated at the configured max_response_chars; the response footer reports the \
         agent's stop_reason and whether truncation occurred."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Self-contained task description for the external agent. Include all needed context — the agent does not see the chat history."
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 10,
                    "maximum": 3600,
                    "description": "Optional per-call timeout override in seconds (default: the configured acp.timeout_secs)."
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let params: AcpDelegateParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse acp_delegate arguments: {e}"))
        })?;
        params.validate().map_err(|e| {
            RockBotError::ToolCallParse(format!("Invalid acp_delegate arguments: {e}"))
        })?;
        if let Some(t) = params.timeout_secs {
            if !(10..=3600).contains(&t) {
                return Err(RockBotError::ToolCallParse(format!(
                    "Invalid acp_delegate arguments: timeout_secs must be in 10..=3600, got {t}"
                )));
            }
        }

        let timeout_secs = params
            .timeout_secs
            .unwrap_or(self.client.config().timeout_secs);
        let result = self.client.prompt(params.prompt.as_str(), timeout_secs).await?;

        let stop_reason = stop_reason_str(&result.stop_reason);
        let mut out = result.text;
        out.push_str(&format!("\n\n---\n[stop_reason: {stop_reason}]"));
        if result.truncated {
            out.push_str(&format!(
                " [output truncated at {} chars]",
                self.client.config().max_response_chars.as_usize()
            ));
        }
        Ok(out)
    }
}

/// Wire-format (snake_case) stop reason names.
fn stop_reason_str(reason: &agent_client_protocol::schema::v1::StopReason) -> &'static str {
    use agent_client_protocol::schema::v1::StopReason;
    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::MaxTurnRequests => "max_turn_requests",
        StopReason::Refusal => "refusal",
        StopReason::Cancelled => "cancelled",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AcpConfig;

    fn make_tool() -> AcpTool {
        AcpTool::new(Arc::new(AcpClient::new(AcpConfig::default())))
    }

    #[test]
    fn test_tool_definition() {
        let tool = make_tool();
        assert_eq!(tool.name(), "acp_delegate");
        assert!(tool.description().contains("ACP"));
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("prompt"))
        );
    }

    #[tokio::test]
    async fn test_execute_invalid_json() {
        let tool = make_tool();
        let result = tool.execute("not json").await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Tool call parse error"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn test_execute_missing_prompt() {
        let tool = make_tool();
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_empty_prompt() {
        let tool = make_tool();
        let result = tool.execute(r#"{"prompt": ""}"#).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Tool call parse error"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn test_execute_timeout_out_of_range() {
        let tool = make_tool();
        let result = tool.execute(r#"{"prompt": "x", "timeout_secs": 5}"#).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timeout_secs must be in 10..=3600"), "unexpected: {err}");

        let result = tool.execute(r#"{"prompt": "x", "timeout_secs": 4000}"#).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timeout_secs must be in 10..=3600"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn test_execute_spawn_failure_surfaces_acp_error() {
        // Command does not exist — lazy spawn on first prompt must fail with
        // an ACP error (not panic), proving spawn errors surface as tool errors.
        let cfg = AcpConfig {
            enabled: true,
            command: "rockbot-nonexistent-acp-agent-binary".into(),
            ..AcpConfig::default()
        };
        let tool = AcpTool::new(Arc::new(AcpClient::new(cfg)));
        let result = tool.execute(r#"{"prompt": "hello"}"#).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("ACP error"), "unexpected: {err}");
        assert!(err.contains("failed to spawn"), "unexpected: {err}");
    }
}
