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
        let api_key = config.api_key.clone();
        if api_key.is_empty() || api_key == "EDITME" {
            return Err(RockBotError::MissingApiKey(config.name.clone()));
        }

        let base_url = config.base_url.trim_end_matches('/').to_string();
        let chat_path = config.chat_path.as_deref().unwrap_or("/chat/completions");
        let full_url = format!("{}{}", base_url, chat_path);

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
        let api_key = config.api_key.clone();
        if api_key.is_empty() || api_key == "EDITME" {
            return Err(RockBotError::MissingApiKey(config.name.clone()));
        }

        let base_url = config.base_url.trim_end_matches('/').to_string();
        let chat_path = config.chat_path.as_deref().unwrap_or("/chat/completions");
        let full_url = format!("{}{}", base_url, chat_path);

        Ok(Self {
            api_key,
            base_url: full_url,
            model: model.into(),
            http_client: client,
        })
    }

    pub(crate) fn parse_response_body(body: &serde_json::Value) -> Result<CompletionResult> {
        let choices = body
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or(RockBotError::NoChoices)?;

        let choice = choices.first().ok_or(RockBotError::NoChoices)?;
        let message = choice
            .get("message")
            .ok_or(RockBotError::EmptyResponse)?;

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
            reasoning_content: None,
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
            500 | 502 | 503 => RockBotError::ServerError {
                status,
                body: msg,
            },
            _ => RockBotError::Provider(format!("HTTP {}: {}", status, msg)),
        }
    }
}

#[async_trait]
impl AiProvider for OpenRouterProvider {
    async fn complete(&self, request: ChatRequest) -> Result<CompletionResult> {
        let body = serde_json::json!({
            "model": request.model,
            "messages": request.messages,
            "stream": request.stream,
        });

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
        if !status.is_success() {
            let status_code = status.as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_http_error(status_code, &body));
        }

        let response_body: serde_json::Value = response.json().await?;
        Self::parse_response_body(&response_body)
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
}
