use async_trait::async_trait;
use tracing::{debug, warn};

use crate::config::ProviderConfig;
use crate::error::{Result, RockBotError};
use crate::provider::AiProvider;
use crate::types::{
    ChatMessage, ChatRequest, CompletionResult, ContentPart, FinishReason, MessageContent,
    ToolCall, UsageInfo,
};

const TOOL_DELIMITER_START: &str = "✿FUNCTION✿";
const TOOL_DELIMITER_ARGS: &str = "✿ARGS✿";
const TOOL_DELIMITER_END: &str = "✿END✿";

pub struct LlamaCppProvider {
    base_url: String,
    model: String,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for LlamaCppProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamaCppProvider")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

impl LlamaCppProvider {
    pub fn new(config: &ProviderConfig, model: impl Into<String>) -> Result<Self> {
        let full_url = config.chat_url();

        Ok(Self {
            base_url: full_url,
            model: model.into(),
            http_client: super::default_http_client(),
        })
    }

    pub fn with_client(
        config: &ProviderConfig,
        model: impl Into<String>,
        client: reqwest::Client,
    ) -> Result<Self> {
        let full_url = config.chat_url();

        Ok(Self {
            base_url: full_url,
            model: model.into(),
            http_client: client,
        })
    }

    fn strip_message_images(mut msg: ChatMessage) -> ChatMessage {
        let MessageContent::Multipart(ref parts) = msg.content else {
            return msg;
        };
        if !parts.iter().any(|p| matches!(p, ContentPart::ImageUrl { .. })) {
            return msg;
        }
        let text = parts
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => text.as_str(),
                ContentPart::ImageUrl { .. } => "[image]",
            })
            .collect::<Vec<_>>()
            .join(" ");
        msg.content = MessageContent::Text(text);
        msg
    }

    fn parse_tool_calls_from_text(text: &str) -> Vec<ToolCall> {
        let mut calls = Vec::new();
        let mut remaining = text;
        let mut call_index = 0;

        while let Some(start) = remaining.find(TOOL_DELIMITER_START) {
            let after_start = &remaining[start + TOOL_DELIMITER_START.len()..];
            let Some(args_marker) = after_start.find(TOOL_DELIMITER_ARGS) else {
                break;
            };
            let name = after_start[..args_marker].trim();
            let after_args = &after_start[args_marker + TOOL_DELIMITER_ARGS.len()..];
            let Some(end_marker) = after_args.find(TOOL_DELIMITER_END) else {
                break;
            };
            let args = after_args[..end_marker].trim();

            let arguments = if serde_json::from_str::<serde_json::Value>(args).is_ok() {
                args.to_string()
            } else {
                warn!(
                    "llamacpp: malformed tool args for '{}', resetting to {{}}",
                    name
                );
                "{}".to_string()
            };

            calls.push(ToolCall {
                id: format!("llamacpp_call_{}", call_index),
                call_type: "function".to_string(),
                function: crate::types::FunctionCall {
                    name: name.to_string(),
                    arguments,
                },
            });

            call_index += 1;
            remaining = &after_args[end_marker + TOOL_DELIMITER_END.len()..];
        }

        calls
    }

    fn strip_tool_delimiters(text: &str) -> String {
        let mut result = text.to_string();
        loop {
            let before = result.clone();
            if let Some(start) = result.find(TOOL_DELIMITER_START) {
                if let Some(end) = result[start..].find(TOOL_DELIMITER_END) {
                    let end_pos = start + end + TOOL_DELIMITER_END.len();
                    result = format!("{}{}", &result[..start], &result[end_pos..]);
                    continue;
                }
            }
            if result == before {
                break;
            }
        }
        result.trim().to_string()
    }

    pub(crate) fn build_request_body(&self, request: &ChatRequest) -> serde_json::Value {
        let messages: Vec<ChatMessage> = request
            .messages
            .iter()
            .map(|m| Self::strip_message_images(m.clone()))
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream,
        });

        if let Some(ref tools) = request.tools {
            body["tools"] = serde_json::json!(tools);
        }

        if let Some(ref tool_choice) = request.tool_choice {
            body["tool_choice"] = tool_choice.clone();
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
                "content_filter" => FinishReason::ContentFilter,
                "insufficient_system_resource" => FinishReason::InsufficientSystemResource,
                _ => FinishReason::Error,
            })
            .unwrap_or(FinishReason::Error);

        let raw_text = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(String::from);

        let native_tool_calls: Vec<ToolCall> = message
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        serde_json::from_value::<ToolCall>(tc.clone())
                            .map_err(|e| {
                                warn!("llamacpp: skipping malformed tool_call in response: {e}");
                            })
                            .ok()
                    })
                    .collect()
            })
            .unwrap_or_default();

        let (text, tool_calls, finish) = if !native_tool_calls.is_empty() {
            (raw_text, native_tool_calls, finish)
        } else if let Some(ref content) = raw_text {
            let parsed_calls = Self::parse_tool_calls_from_text(content);
            if !parsed_calls.is_empty() {
                let clean_text = Self::strip_tool_delimiters(content);
                let clean_text = if clean_text.is_empty() {
                    None
                } else {
                    Some(clean_text)
                };
                (clean_text, parsed_calls, FinishReason::ToolUse)
            } else {
                (raw_text, vec![], finish)
            }
        } else {
            (raw_text, vec![], finish)
        };

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
            reasoning_content: None,
            usage,
        })
    }

    pub(crate) fn map_http_error(status: u16, body: &str) -> RockBotError {
        let msg = extract_error_message(body);

        if status == 400 && is_context_length_error(&msg) {
            return RockBotError::ContextLengthExceeded(msg);
        }

        match status {
            400 => RockBotError::InvalidRequest(msg),
            401 => RockBotError::AuthFailed(msg),
            422 => RockBotError::InvalidParameters(msg),
            500 | 502 | 503 => RockBotError::ServerError {
                status,
                body: msg,
            },
            _ => RockBotError::Provider(format!("HTTP {}: {}", status, msg)),
        }
    }
}

fn extract_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| body.to_string())
}

fn is_context_length_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("context length") || lower.contains("maximum context")
}

#[async_trait]
impl AiProvider for LlamaCppProvider {
    async fn complete(&self, mut request: ChatRequest) -> Result<CompletionResult> {
        for msg in &mut request.messages {
            msg.reasoning_content = None;
        }
        let body = self.build_request_body(&request);
        let msg_count = request.messages.len();
        let tool_count = request.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        debug!(
            "llama.cpp request: model={} messages={} tools={} stream={}",
            request.model, msg_count, tool_count, request.stream
        );

        let response = self
            .http_client
            .post(&self.base_url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| RockBotError::ServerError {
                status: 0,
                body: format!("Connection failed: {}", e),
            })?;

        let status = response.status();
        if status.is_success() {
            let response_body: serde_json::Value = response.json().await?;
            let tool_count = response_body
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("tool_calls"))
                .and_then(|t| t.as_array())
                .map(|t| t.len())
                .unwrap_or(0);
            let text_len = response_body
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .map(|s| s.len())
                .unwrap_or(0);
            debug!(
                "llama.cpp response: finish={:?} text_len={} tool_calls={}",
                response_body
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|c| c.get("finish_reason"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("unknown"),
                text_len,
                tool_count,
            );
            return Self::parse_response_body(&response_body);
        }

        let status_code = status.as_u16();
        let error_body = response.text().await.unwrap_or_default();
        warn!(
            "llama.cpp HTTP {}: error_body={}",
            status_code, &error_body
        );
        Err(Self::map_http_error(status_code, &error_body))
    }

    fn provider_name(&self) -> &str {
        "llamacpp"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, ThinkingConfig, ToolDef};
    use crate::validated::{ConfigUrl, ProviderName};

    fn test_provider() -> LlamaCppProvider {
        LlamaCppProvider {
            base_url: "http://localhost:8080/v1/chat/completions".into(),
            model: "local-model".into(),
            http_client: reqwest::Client::new(),
        }
    }

    #[test]
    fn test_new_empty_api_key_accepted() {
        let config = ProviderConfig {
            name: ProviderName::try_new("llamacpp".to_string()).unwrap(),
            api_key: "".into(),
            base_url: ConfigUrl::try_new("http://localhost:8080/v1".to_string()).unwrap(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let result = LlamaCppProvider::new(&config, "local-model");
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_request_body_minimal() {
        let provider = test_provider();
        let request = ChatRequest {
            model: "local-model".into(),
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
        assert_eq!(body["model"], "local-model");
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
        assert!(body.get("tools").is_none());
        assert!(body.get("thinking").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_build_request_body_strips_images() {
        let provider = test_provider();
        let request = ChatRequest {
            model: "local-model".into(),
            messages: vec![ChatMessage::user_with_image(
                "Look at this",
                "data:image/png;base64,abc",
            )],
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(
            body["messages"][0]["content"],
            "Look at this [image]"
        );
    }

    #[test]
    fn test_build_request_body_tools_native() {
        let provider = test_provider();
        let request = ChatRequest {
            model: "local-model".into(),
            messages: vec![
                ChatMessage::system("You are helpful"),
                ChatMessage::user("What's the weather?"),
            ],
            tools: Some(vec![ToolDef::new(
                "get_weather",
                "Get weather for a city",
                serde_json::json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            )]),
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        let system_content = body["messages"][0]["content"].as_str().unwrap();
        assert_eq!(system_content, "You are helpful");
        let tools = body.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(
            tools[0]["function"]["description"],
            "Get weather for a city"
        );
    }

    #[test]
    fn test_build_request_body_tools_no_system_message() {
        let provider = test_provider();
        let request = ChatRequest {
            model: "local-model".into(),
            messages: vec![ChatMessage::user("What's the weather?")],
            tools: Some(vec![ToolDef::new(
                "get_weather",
                "Get weather",
                serde_json::json!({"type": "object"}),
            )]),
            stream: false,
            temperature: None,
            max_tokens: None,
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        let tools = body.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools[0]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_build_request_body_omits_thinking() {
        let provider = test_provider();
        let request = ChatRequest {
            model: "local-model".into(),
            messages: vec![ChatMessage::user("Hello")],
            tools: None,
            stream: false,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            thinking: Some(ThinkingConfig::enabled()),
            reasoning_effort: Some("high".into()),
            tool_choice: None,
        };

        let body = provider.build_request_body(&request);
        assert!(body.get("thinking").is_none());
        assert!(body.get("reasoning_effort").is_none());
        let temp = body["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.001);
        assert_eq!(body["max_tokens"], 2048);
    }

    #[test]
    fn test_parse_response_simple() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let result = LlamaCppProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.text, Some("Hello! How can I help?".into()));
        assert_eq!(result.finish, FinishReason::Stop);
        assert!(result.tool_calls.is_empty());
        assert!(result.reasoning_content.is_none());
        assert_eq!(result.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn test_parse_response_native_tool_calls() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_001",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\": \"Tokyo\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = LlamaCppProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.finish, FinishReason::ToolUse);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].function.name, "get_weather");
    }

    #[test]
    fn test_parse_response_text_tool_calls() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Let me check that.\n✿FUNCTION✿get_weather✿ARGS✿{\"city\": \"Tokyo\"}✿END✿"
                },
                "finish_reason": "stop"
            }]
        });

        let result = LlamaCppProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.finish, FinishReason::ToolUse);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].function.name, "get_weather");
        assert_eq!(
            result.tool_calls[0].function.arguments,
            "{\"city\": \"Tokyo\"}"
        );
        assert_eq!(result.text, Some("Let me check that.".into()));
    }

    #[test]
    fn test_parse_response_multiple_text_tool_calls() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "✿FUNCTION✿search✿ARGS✿{\"q\": \"rust\"}✿END✿\n✿FUNCTION✿fetch✿ARGS✿{\"url\": \"https://example.com\"}✿END✿"
                },
                "finish_reason": "stop"
            }]
        });

        let result = LlamaCppProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.tool_calls[0].function.name, "search");
        assert_eq!(result.tool_calls[1].function.name, "fetch");
    }

    #[test]
    fn test_parse_response_malformed_tool_args() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "✿FUNCTION✿get_weather✿ARGS✿not-json✿END✿"
                },
                "finish_reason": "stop"
            }]
        });

        let result = LlamaCppProvider::parse_response_body(&json).unwrap();
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].function.arguments, "{}");
    }

    #[test]
    fn test_parse_response_no_choices() {
        let json = serde_json::json!({});
        let result = LlamaCppProvider::parse_response_body(&json);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_http_error_context_length() {
        let err = LlamaCppProvider::map_http_error(
            400,
            r#"{"error": {"message": "This model's maximum context length is 4096 tokens"}}"#,
        );
        assert!(matches!(err, RockBotError::ContextLengthExceeded(_)));
    }

    #[test]
    fn test_map_http_error_500() {
        let err = LlamaCppProvider::map_http_error(500, "Internal error");
        assert!(matches!(err, RockBotError::ServerError { status: 500, .. }));
    }

    #[test]
    fn test_map_http_error_401() {
        let err = LlamaCppProvider::map_http_error(401, r#"{"error": {"message": "Bad key"}}"#);
        match err {
            RockBotError::AuthFailed(msg) => assert_eq!(msg, "Bad key"),
            _ => panic!("Expected AuthFailed"),
        }
    }

    #[test]
    fn test_provider_name_and_model() {
        let provider = test_provider();
        assert_eq!(provider.provider_name(), "llamacpp");
        assert_eq!(provider.model_name(), "local-model");
    }

    #[test]
    fn test_strip_message_images() {
        let msg = ChatMessage::user_with_image("Look", "data:image/png;base64,abc");
        let result = LlamaCppProvider::strip_message_images(msg);
        assert_eq!(result.content, MessageContent::Text("Look [image]".into()));
    }

    #[test]
    fn test_parse_tool_calls_from_text_none() {
        let calls = LlamaCppProvider::parse_tool_calls_from_text("Just a normal reply");
        assert!(calls.is_empty());
    }

    #[test]
    fn test_strip_tool_delimiters() {
        let text = "Before\n✿FUNCTION✿fn✿ARGS✿{}✿END✿\nAfter";
        let result = LlamaCppProvider::strip_tool_delimiters(text);
        assert_eq!(result, "Before\n\nAfter");
    }

    #[test]
    fn test_chat_url_default() {
        let config = ProviderConfig {
            name: ProviderName::try_new("llamacpp".to_string()).unwrap(),
            api_key: "".into(),
            base_url: ConfigUrl::try_new("http://localhost:8080/v1".to_string()).unwrap(),
            basecf_url: None,
            chat_path: None,
            draw_path: None,
            models: std::collections::HashMap::new(),
        };
        let provider = LlamaCppProvider::new(&config, "local").unwrap();
        assert_eq!(
            provider.base_url,
            "http://localhost:8080/v1/chat/completions"
        );
    }
}
