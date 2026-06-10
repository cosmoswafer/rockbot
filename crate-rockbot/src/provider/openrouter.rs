use async_trait::async_trait;

use crate::config::ProviderConfig;
use crate::error::{Result, RockBotError};
use crate::provider::AiProvider;
use crate::types::{ChatRequest, CompletionResult, FinishReason, ToolCall, UsageInfo};

pub struct OpenRouterProvider {
    api_key: String,
    base_url: String,
    model: String,
    #[allow(dead_code)]
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OpenRouterProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterProvider")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

impl OpenRouterProvider {
    pub fn new(config: &ProviderConfig, model: impl Into<String>) -> Result<Self> {
        config.validate_api_key()?;
        let api_key = config.api_key.clone();
        let full_url = config.chat_url();

        Ok(Self {
            api_key,
            base_url: full_url,
            model: model.into(),
            http_client: reqwest::Client::new(),
        })
    }

    pub fn with_client(
        config: &ProviderConfig,
        model: impl Into<String>,
        client: reqwest::Client,
    ) -> Result<Self> {
        config.validate_api_key()?;
        let api_key = config.api_key.clone();
        let full_url = config.chat_url();

        Ok(Self {
            api_key,
            base_url: full_url,
            model: model.into(),
            http_client: client,
        })
    }

    pub(crate) fn build_request_body(&self, request: &ChatRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": request.model,
            "messages": request.messages,
            "stream": request.stream,
        });

        if let Some(ref tools) = request.tools {
            body["tools"] = serde_json::to_value(tools).unwrap();
        }

        if let Some(ref tool_choice) = request.tool_choice {
            body["tool_choice"] = tool_choice.clone();
        }

        if let Some(ref thinking) = request.thinking {
            body["thinking"] = serde_json::json!({
                "type": thinking.thinking_type
            });
        }

        if let Some(ref effort) = request.reasoning_effort {
            body["reasoning_effort"] = serde_json::Value::String(effort.clone());
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if let Some(max_tok) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tok);
        }

        body
    }

    pub(crate) fn parse_response_body(body: &serde_json::Value) -> Result<CompletionResult> {
        let choices = body
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or(RockBotError::NoChoices)?;

        let choice = choices.first().ok_or(RockBotError::NoChoices)?;
        let message = choice.get("message").ok_or(RockBotError::EmptyResponse)?;

        let finish = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .map(|s| match s {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "tool_calls" => FinishReason::ToolUse,
                _ => FinishReason::Error,
            })
            .unwrap_or(FinishReason::Error);

        let text = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(String::from);

        let reasoning_content = message
            .get("reasoning_content")
            .and_then(|r| r.as_str())
            .map(String::from);

        let tool_calls: Vec<ToolCall> = message
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| serde_json::from_value(tc.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        let usage = body.get("usage").and_then(|u| {
            Some(UsageInfo {
                prompt_tokens: u.get("prompt_tokens")?.as_u64()?,
                completion_tokens: u.get("completion_tokens")?.as_u64()?,
                total_tokens: u.get("total_tokens")?.as_u64()?,
            })
        });

        Ok(CompletionResult {
            text,
            tool_calls,
            finish,
            reasoning_content,
            usage,
        })
    }

    fn map_http_error(status: u16, body: &str) -> RockBotError {
        let msg = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| body.to_string());

        match status {
            401 => RockBotError::AuthFailed(msg),
            429 => RockBotError::RateLimited { retry_after: None },
            500 | 502 | 503 => RockBotError::ServerError { status, body: msg },
            _ => RockBotError::Provider(format!("HTTP {}: {}", status, msg)),
        }
    }
}

#[async_trait]
impl AiProvider for OpenRouterProvider {
    async fn complete(&self, request: ChatRequest) -> Result<CompletionResult> {
        let body = self.build_request_body(&request);
        let max_retries: u32 = 3;

        for attempt in 0..=max_retries {
            let response = self
                .http_client
                .post(&self.base_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .header("HTTP-Referer", "https://github.com/anomalyco/rockbot")
                .header("X-Title", "RockBot")
                .json(&body)
                .send()
                .await?;

            let status = response.status();
            if status.is_success() {
                let response_body: serde_json::Value = response.json().await?;
                return Self::parse_response_body(&response_body);
            }

            let status_code = status.as_u16();
            let error_body = response.text().await.unwrap_or_default();

            if (status_code == 429 || status_code >= 500) && attempt < max_retries {
                let delay = 2u64.pow(attempt + 1);
                tracing::warn!(
                    "OpenRouter HTTP {}, retrying in {}s (attempt {}/{})",
                    status_code,
                    delay,
                    attempt + 1,
                    max_retries
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                continue;
            }

            return Err(Self::map_http_error(status_code, &error_body));
        }

        unreachable!("retry loop exhausted")
    }

    fn provider_name(&self) -> &str {
        "openrouter"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, ThinkingConfig, ToolDef};

    fn make_provider(model: &str) -> OpenRouterProvider {
        let config = ProviderConfig {
            name: "openrouter".into(),
            api_key: "sk-or-v1-test".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            basecf_url: None,
            chat_path: Some("/chat/completions".into()),
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        OpenRouterProvider::new(&config, model).unwrap()
    }

    #[test]
    fn test_build_request_body_minimal() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Hello")],
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["model"], "openai/gpt-4");
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
        assert!(body.get("tools").is_none());
        assert!(body.get("temperature").is_none());
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn test_build_request_body_with_tools() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Weather?")],
            tools: Some(vec![ToolDef::new(
                "get_weather",
                "Get weather",
                serde_json::json!({"type": "object", "properties": {}}),
            )]),
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert!(body["tools"].is_array());
        assert_eq!(body["tools"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_build_request_body_with_temperature_and_max_tokens() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Hi")],
            tools: None,
            stream: false,
            temperature: Some(0.5),
            max_tokens: Some(1024),
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["max_tokens"], 1024);
    }

    #[test]
    fn test_build_request_body_with_thinking_enabled() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Think about it")],
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: Some(ThinkingConfig::enabled()),
            reasoning_effort: Some("medium".into()),
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "medium");
    }

    #[test]
    fn test_build_request_body_with_thinking_disabled() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("No think")],
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: Some(ThinkingConfig::disabled()),
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_build_request_body_with_tool_choice() {
        let provider = make_provider("openai/gpt-4");
        let request = ChatRequest {
            model: "openai/gpt-4".into(),
            messages: vec![ChatMessage::user("Force tool")],
            tools: Some(vec![ToolDef::new(
                "calc",
                "Calculator",
                serde_json::json!({"type": "object", "properties": {}}),
            )]),
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: Some(serde_json::json!("auto")),
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn test_parse_response_simple() {
        let json = serde_json::json!({
            "id": "or-123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello from OpenRouter!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let result = OpenRouterProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.text, Some("Hello from OpenRouter!".into()));
        assert_eq!(result.finish, FinishReason::Stop);
        assert_eq!(result.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn test_parse_response_with_tool_calls() {
        let json = serde_json::json!({
            "id": "or-456",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_001",
                        "type": "function",
                        "function": {
                            "name": "web_search",
                            "arguments": "{\"query\": \"rust\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = OpenRouterProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.finish, FinishReason::ToolUse);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].function.name, "web_search");
    }

    #[test]
    fn test_parse_response_with_reasoning() {
        let json = serde_json::json!({
            "id": "or-789",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "The answer is 42",
                    "reasoning_content": "Let me think..."
                },
                "finish_reason": "stop"
            }]
        });

        let result = OpenRouterProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.text, Some("The answer is 42".into()));
        assert_eq!(result.reasoning_content, Some("Let me think...".into()));
    }

    #[test]
    fn test_parse_response_length_finish() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Truncated..."
                },
                "finish_reason": "length"
            }]
        });

        let result = OpenRouterProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.finish, FinishReason::Length);
    }

    #[test]
    fn test_parse_response_no_choices() {
        let json = serde_json::json!({});
        let result = OpenRouterProvider::parse_response_body(&json);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_http_error_401() {
        let err = OpenRouterProvider::map_http_error(
            401,
            r#"{"error": {"message": "Invalid credentials"}}"#,
        );
        match err {
            RockBotError::AuthFailed(msg) => assert_eq!(msg, "Invalid credentials"),
            _ => panic!("Expected AuthFailed"),
        }
    }

    #[test]
    fn test_map_http_error_429() {
        let err = OpenRouterProvider::map_http_error(429, "");
        match err {
            RockBotError::RateLimited { .. } => {}
            _ => panic!("Expected RateLimited"),
        }
    }

    #[test]
    fn test_map_http_error_500() {
        let err = OpenRouterProvider::map_http_error(500, "Server boom");
        match err {
            RockBotError::ServerError { status, .. } => assert_eq!(status, 500),
            _ => panic!("Expected ServerError"),
        }
    }

    #[test]
    fn test_new_missing_api_key() {
        let config = ProviderConfig {
            name: "openrouter".into(),
            api_key: "EDITME".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let result = OpenRouterProvider::new(&config, "openai/gpt-4");
        assert!(result.is_err());
    }

    #[test]
    fn test_new_empty_api_key() {
        let config = ProviderConfig {
            name: "openrouter".into(),
            api_key: "".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let result = OpenRouterProvider::new(&config, "gpt");
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_name_and_model() {
        let provider = make_provider("openai/gpt-4o");
        assert_eq!(provider.provider_name(), "openrouter");
        assert_eq!(provider.model_name(), "openai/gpt-4o");
    }

    #[test]
    fn test_chat_url_custom_path() {
        let config = ProviderConfig {
            name: "openrouter".into(),
            api_key: "sk-test".into(),
            base_url: "https://custom.api.com".into(),
            basecf_url: None,
            chat_path: Some("/v2/chat".into()),
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let provider = OpenRouterProvider::new(&config, "model").unwrap();
        assert_eq!(provider.base_url, "https://custom.api.com/v2/chat");
    }

    #[test]
    fn test_with_client() {
        let config = ProviderConfig {
            name: "openrouter".into(),
            api_key: "sk-test".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let client = reqwest::Client::new();
        let provider = OpenRouterProvider::with_client(&config, "openai/gpt-4", client).unwrap();
        assert_eq!(provider.model_name(), "openai/gpt-4");
    }
}
