use rockbot::config::ProviderConfig;
use rockbot::error::RockBotError;
use rockbot::validated::{ConfigUrl, NonEmptyString, ProviderName};
    use rockbot::provider::{AiProvider, DeepSeekProvider, FalAiProvider, ImageProvider, LlamaCppProvider, OpenRouterImageProvider, OpenRouterProvider};
    use rockbot::tool::Tool;
    use rockbot::tools::ImageGenTool;
    use rockbot::types::{ChatMessage, ChatRequest, FinishReason, ImageGenParams, ImageModelCatalog, ThinkingConfig, ToolCall, ToolDef};
    use std::collections::HashMap;
    use std::sync::Arc;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

/// Custom matcher asserting the request carries NO `authorization` header.
struct NoAuthHeader;

impl Match for NoAuthHeader {
    fn matches(&self, request: &Request) -> bool {
        request.headers.get("authorization").is_none()
    }
}

// ─── Mock HTTP Tests: DeepSeekProvider.complete() ────────────────────────────

#[tokio::test]
async fn test_complete_simple_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("Authorization", "Bearer sk-test-key"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-001",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! I'm DeepSeek."
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-test-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Hello")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("Hello! I'm DeepSeek.".into()));
    assert_eq!(result.finish, FinishReason::Stop);
    assert!(result.tool_calls.is_empty());
}

#[tokio::test]
async fn test_deepseek_vision_model_forwards_images_in_user_messages() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("Authorization", "Bearer sk-test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-vision",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "The image is blue."},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-test-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-flash-vision-exp").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-flash-vision-exp".into(),
        messages: vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user_with_images(
                "Attached: ![a.png](a.png)",
                vec!["data:image/png;base64,abc".into()],
            ),
        ],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("The image is blue.".into()));

    // The wire body must keep the ImageUrl part in the user message and must
    // NOT contain the [image] placeholder conversion.
    let received = mock_server.received_requests().await.unwrap();
    let sent: serde_json::Value =
        serde_json::from_slice(&received[0].body).expect("request body is JSON");
    let parts = sent["messages"][1]["content"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1]["type"], "image_url");
    assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,abc");
    assert_eq!(parts[1]["image_url"]["detail"], "high");
    assert!(sent["messages"][1]["content"].to_string().contains("image_url"));
    assert!(received[0].body.windows(7).all(|w| w != b"[image]"));
}

#[tokio::test]
async fn test_deepseek_text_model_strips_images_before_send() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("Authorization", "Bearer sk-test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-text",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "OK"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-test-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user_with_images(
            "Look at this",
            vec!["data:image/png;base64,abc".into()],
        )],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    provider.complete(request).await.unwrap();

    // Text-only model: ImageUrl is stripped to a plain [image] placeholder —
    // no multipart content array survives on the wire.
    let received = mock_server.received_requests().await.unwrap();
    let sent: serde_json::Value =
        serde_json::from_slice(&received[0].body).expect("request body is JSON");
    assert_eq!(sent["messages"][0]["content"], "Look at this [image]");
    assert!(sent["messages"][0]["content"].is_string());
    assert!(!sent.to_string().contains("image_url"));
}

#[tokio::test]
async fn test_complete_with_tool_calls() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"Tokyo\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Weather in Tokyo?")],
        tools: Some(vec![ToolDef::new(
            "get_weather",
            "Get weather for a location",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        )]),
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.finish, FinishReason::ToolUse);
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].function.name, "get_weather");
}

/// Gitea #80 — request-side: malformed tool call arguments in history are
/// repaired (string-aware) before the request reaches the provider.
#[tokio::test]
async fn test_deepseek_repairs_truncated_history_tool_args_before_send() {
    let mock_server = MockServer::start().await;

    // Matcher: the request body must contain a REPAIRED arguments document —
    // parseable, with the truncated string closed and the object balanced.
    struct RepairedToolArgsMatcher;
    #[async_trait::async_trait]
    impl Match for RepairedToolArgsMatcher {
        fn matches(&self, request: &Request) -> bool {
            let body: serde_json::Value = match serde_json::from_slice(&request.body) {
                Ok(v) => v,
                Err(_) => return false,
            };
            let Some(args) = body
                .get("messages")
                .and_then(|m| m.as_array())
                .and_then(|arr| {
                    arr.iter().find(|m| {
                        m.get("tool_calls").and_then(|t| t.as_array()).is_some()
                    })
                })
                .and_then(|m| {
                    m.get("tool_calls")
                        .and_then(|t| t.as_array())
                        .and_then(|arr| arr.first())
                })
                .and_then(|tc| tc.get("function"))
                .and_then(|f| f.get("arguments"))
                .and_then(|a| a.as_str())
            else {
                return false;
            };
            serde_json::from_str::<serde_json::Value>(args).is_ok()
                && args.contains("FHIR report")
                && args.ends_with('}')
        }
    }

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(RepairedToolArgsMatcher)
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "done" },
                "finish_reason": "stop"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    // History contains a truncated 11K-char-style report argument (missing
    // closing quote, embedded JSON braces).
    let truncated = r#"{"content":"FHIR report
```json
{\"resourceType\": \"Patient\", \"id\": \"1\"}
```
more text"#;
    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::assistant_with_tool_calls(
            "",
            vec![ToolCall::new("call_001", "save_knowledge", truncated.to_string())],
            None,
        )],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("done".into()));
}

/// Gitea #80 — response-side: truncated tool call arguments in the provider
/// response are repaired before entering conversation history.
#[tokio::test]
async fn test_deepseek_response_truncated_tool_args_repaired() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_truncated",
                        "type": "function",
                        "function": {
                            "name": "save_knowledge",
                            "arguments": "{\"content\":\"FHIR report truncated mid-..."
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Write a report")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.tool_calls.len(), 1);
    let args = &result.tool_calls[0].function.arguments;
    serde_json::from_str::<serde_json::Value>(args)
        .expect("repaired response args must parse");
    assert!(args.contains("FHIR report truncated mid-"));
}

#[tokio::test]
async fn test_complete_with_reasoning() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "The answer is 42.",
                    "reasoning_content": "Let me think step by step..."
                },
                "finish_reason": "stop"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("What is the answer?")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: Some(ThinkingConfig::enabled()),
        reasoning_effort: Some("high".into()),
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("The answer is 42.".into()));
    assert_eq!(
        result.reasoning_content,
        Some("Let me think step by step...".into())
    );
}

#[tokio::test]
async fn test_complete_401_unauthorized() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"error": {"message": "Invalid API key"}})),
        )
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-bad-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    assert!(matches!(err, RockBotError::AuthFailed(_)));
}

#[tokio::test]
async fn test_complete_429_rate_limit() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(serde_json::json!({"error": {"message": "Rate limit exceeded"}})),
        )
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    assert!(matches!(err, RockBotError::RateLimited { .. }));
}

#[tokio::test]
async fn test_complete_500_server_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    match err {
        RockBotError::ServerError { status, .. } => assert_eq!(status, 500),
        _ => panic!("Expected ServerError, got {:?}", err),
    }
}

#[tokio::test]
async fn test_complete_503_overloaded() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    match err {
        RockBotError::ServerError { status, .. } => assert_eq!(status, 503),
        _ => panic!("Expected ServerError"),
    }
}

#[tokio::test]
async fn test_complete_402_insufficient_balance() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(402)
                .set_body_json(serde_json::json!({"error": {"message": "Insufficient balance"}})),
        )
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    assert!(matches!(err, RockBotError::InsufficientBalance));
}

#[tokio::test]
async fn test_complete_with_thinking_and_tools() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "The weather in Beijing is sunny.",
                    "reasoning_content": "User wants Beijing weather. I need to call the tool first."
                },
                "finish_reason": "stop"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Weather in Beijing?")],
        tools: Some(vec![ToolDef::new(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        )]),
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: Some(ThinkingConfig::enabled()),
        reasoning_effort: Some("high".into()),
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("The weather in Beijing is sunny.".into()));
    assert!(result.reasoning_content.is_some());
}

#[tokio::test]
async fn test_complete_custom_chat_path() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Custom path works!"
                },
                "finish_reason": "stop"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/v1/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-flash").unwrap();

    let request = ChatRequest {
        model: "deepseek-v4-flash".into(),
        messages: vec![ChatMessage::user("Test")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("Custom path works!".into()));
}

#[tokio::test]
async fn test_complete_multi_turn_conversation() {
    let mock_server = MockServer::start().await;

    let mock_response = serde_json::json!({
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "The sum is 42."
            },
            "finish_reason": "stop"
        }]
    });

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();

    let messages = vec![
        ChatMessage::system("You are a helpful math tutor."),
        ChatMessage::user("What is 21 + 21?"),
        ChatMessage::assistant("21 + 21 = 42"),
        ChatMessage::user("And what is 21 * 2?"),
    ];

    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages,
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert!(result.text.is_some());
}

#[tokio::test]
async fn test_complete_422_invalid_params() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(serde_json::json!({"error": {"message": "Invalid model name"}})),
        )
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "invalid-model").unwrap();

    let request = ChatRequest {
        model: "invalid-model".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    assert!(matches!(err, RockBotError::InvalidParameters(_)));
}

// ─── Mock HTTP Tests: OpenRouterProvider.complete() ─────────────────────────

#[tokio::test]
async fn test_openrouter_complete_simple_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("Authorization", "Bearer sk-or-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "or-001",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello from OpenRouter!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 8,
                "completion_tokens": 4,
                "total_tokens": 12
            }
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-or-test".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = OpenRouterProvider::new(&config, "openai/gpt-4").unwrap();

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

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("Hello from OpenRouter!".into()));
    assert_eq!(result.finish, FinishReason::Stop);
}

#[tokio::test]
async fn test_openrouter_complete_with_tools() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_or1",
                        "type": "function",
                        "function": {
                            "name": "web_search",
                            "arguments": "{\"query\":\"rust lang\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-or-test".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = OpenRouterProvider::new(&config, "openai/gpt-4").unwrap();

    let request = ChatRequest {
        model: "openai/gpt-4".into(),
        messages: vec![ChatMessage::user("Search for rust")],
        tools: Some(vec![ToolDef::new(
            "web_search",
            "Search the web",
            serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        )]),
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.finish, FinishReason::ToolUse);
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].function.name, "web_search");
}

#[tokio::test]
async fn test_openrouter_complete_with_temperature() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Creative response"
                },
                "finish_reason": "stop"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-or-test".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = OpenRouterProvider::new(&config, "openai/gpt-4").unwrap();

    let request = ChatRequest {
        model: "openai/gpt-4".into(),
        messages: vec![ChatMessage::user("Be creative")],
        tools: None,
        stream: false,
        temperature: Some(0.9),
        max_tokens: Some(2048),
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert!(result.text.is_some());
}

#[tokio::test]
async fn test_openrouter_complete_401() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"error": {"message": "Bad API key"}})),
        )
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-bad".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = OpenRouterProvider::new(&config, "openai/gpt-4").unwrap();

    let request = ChatRequest {
        model: "openai/gpt-4".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    assert!(matches!(err, RockBotError::AuthFailed(_)));
}

#[tokio::test]
async fn test_openrouter_complete_429() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(serde_json::json!({"error": {"message": "Too many requests"}})),
        )
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-or-test".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = OpenRouterProvider::new(&config, "openai/gpt-4").unwrap();

    let request = ChatRequest {
        model: "openai/gpt-4".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    assert!(matches!(err, RockBotError::RateLimited { .. }));
}

#[tokio::test]
async fn test_openrouter_complete_500() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Server error"))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-or-test".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = OpenRouterProvider::new(&config, "openai/gpt-4").unwrap();

    let request = ChatRequest {
        model: "openai/gpt-4".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    match err {
        RockBotError::ServerError { status, .. } => assert_eq!(status, 500),
        _ => panic!("Expected ServerError"),
    }
}

#[tokio::test]
async fn test_openrouter_complete_with_reasoning() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Paris is the capital of France.",
                    "reasoning_content": "The user asked about the capital of France. France's capital is Paris."
                },
                "finish_reason": "stop"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-or-test".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = OpenRouterProvider::new(&config, "openai/gpt-4").unwrap();

    let request = ChatRequest {
        model: "openai/gpt-4".into(),
        messages: vec![ChatMessage::user("What is the capital of France?")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: Some(ThinkingConfig::enabled()),
        reasoning_effort: Some("high".into()),
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert!(result.text.is_some());
    assert!(result.reasoning_content.is_some());
}

// ─── Mock HTTP Tests: WebDavTool ──────────────────────────────────────────────

fn make_test_client(mock_uri: &str) -> webdav::WebDavClient {
    webdav::WebDavClient::new(mock_uri, "testuser", "testpass").unwrap()
}

fn propfind_xml_response(href: &str, _name: &str, size: u64, modified: &str) -> String {
    format!(
        r#"  <response>
    <href>{href}</href>
    <propstat>
      <prop>
        <getlastmodified>{modified}</getlastmodified>
        <getcontentlength>{size}</getcontentlength>
        <resourcetype></resourcetype>
      </prop>
    </propstat>
  </response>"#
    )
}

fn propfind_xml_body(responses: &[String]) -> String {
    let responses_xml = responses.join("\n");
    format!(
        r#"<?xml version="1.0"?>
<multistatus>
{responses_xml}
</multistatus>"#
    )
}

#[tokio::test]
async fn test_webdav_read() {
    let mock_server = MockServer::start().await;
    let file_content = "Hello, WebDAV!";

    Mock::given(method("GET"))
        .and(path("/general/notes.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(file_content))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(r#"{"action": "read", "room_id": "general", "path": "notes.txt"}"#)
        .await
        .unwrap();
    assert_eq!(result, file_content);
}

#[tokio::test]
async fn test_webdav_write() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/general/newnotes.txt"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "write", "room_id": "general", "path": "newnotes.txt", "content": "new content"}"#,
        )
        .await
        .unwrap();
    assert!(result.contains("bytes"));
    assert!(result.contains("general/newnotes.txt"));
}

#[tokio::test]
async fn test_webdav_list_empty() {
    let mock_server = MockServer::start().await;

    let empty_xml = r#"<?xml version="1.0"?>
<multistatus />"#;

    Mock::given(method("PROPFIND"))
        .and(header("Depth", "1"))
        .respond_with(ResponseTemplate::new(207).set_body_string(empty_xml))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(r#"{"action": "list", "room_id": "general", "path": ""}"#)
        .await
        .unwrap();
    assert!(result.contains("empty"));
}

#[tokio::test]
async fn test_webdav_list_with_entries() {
    let mock_server = MockServer::start().await;

    let responses = vec![propfind_xml_response(
        "/general/notes.txt",
        "notes.txt",
        2048,
        "Mon, 01 Jan 2024 00:00:00 GMT",
    )];
    let xml = propfind_xml_body(&responses);

    Mock::given(method("PROPFIND"))
        .and(header("Depth", "1"))
        .respond_with(ResponseTemplate::new(207).set_body_string(xml))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(r#"{"action": "list", "room_id": "general", "path": ""}"#)
        .await
        .unwrap();
    assert!(result.contains("notes.txt"));
    assert!(result.contains("2.0 KB"));
}

#[tokio::test]
async fn test_webdav_mkdir() {
    let mock_server = MockServer::start().await;

    Mock::given(method("MKCOL"))
        .and(path("/general"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&mock_server)
        .await;
    Mock::given(method("MKCOL"))
        .and(path("/general/workspace"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(r#"{"action": "mkdir", "room_id": "general", "path": "workspace"}"#)
        .await
        .unwrap();
    assert!(result.contains("created"));
}

#[tokio::test]
async fn test_webdav_delete() {
    let mock_server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/general/old.txt"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(r#"{"action": "delete", "room_id": "general", "path": "old.txt"}"#)
        .await
        .unwrap();
    assert!(result.contains("Deleted"));
}

#[tokio::test]
async fn test_webdav_exists_true() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PROPFIND"))
        .and(header("Depth", "0"))
        .respond_with(ResponseTemplate::new(207))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(r#"{"action": "exists", "room_id": "general", "path": "notes.txt"}"#)
        .await
        .unwrap();
    assert!(result.contains("exists"));
}

#[tokio::test]
async fn test_webdav_exists_false() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PROPFIND"))
        .and(header("Depth", "0"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(r#"{"action": "exists", "room_id": "general", "path": "missing.txt"}"#)
        .await
        .unwrap();
    assert!(result.contains("not found"));
}

#[tokio::test]
async fn test_webdav_mkdir_deep() {
    let mock_server = MockServer::start().await;

    let dirs = vec!["/general", "/general/sub", "/general/sub/deep"];
    for dir in dirs {
        Mock::given(method("MKCOL"))
            .and(path(dir))
            .respond_with(ResponseTemplate::new(201))
            .mount(&mock_server)
            .await;
    }

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(r#"{"action": "mkdir", "room_id": "general", "path": "sub/deep"}"#)
        .await
        .unwrap();
    assert!(result.contains("created"));
}

// ─── WebDAV Write-With-Fallback Tests ──────────────────────────────────────────

#[tokio::test]
async fn test_webdav_write_fallback_happy_path() {
    let mock_server = MockServer::start().await;

    // AutoMkcol succeeds on first try
    Mock::given(method("PUT"))
        .and(path("/general/notes.txt"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "write", "room_id": "general", "path": "notes.txt", "content": "hello"}"#,
        )
        .await
        .unwrap();
    assert!(result.contains("bytes"));
    assert!(result.contains("general/notes.txt"));
}

#[tokio::test]
async fn test_webdav_write_fallback_404_then_mkdir_retry() {
    let mock_server = MockServer::start().await;

    // AutoMkcol returns 404 (server doesn't support it / parent dir missing)
    Mock::given(method("PUT"))
        .and(path("/general/notes.txt"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&mock_server)
        .await;

    // ensure_directory_all creates /general (root dir already exists via 405, just return 201)
    Mock::given(method("MKCOL"))
        .and(path("/general"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    // Retry plain PUT succeeds
    Mock::given(method("PUT"))
        .and(path("/general/notes.txt"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "write", "room_id": "general", "path": "notes.txt", "content": "hello"}"#,
        )
        .await
        .unwrap();
    assert!(result.contains("bytes"));
    assert!(result.contains("general/notes.txt"));
}

#[tokio::test]
async fn test_webdav_write_fallback_nested_dir_creation() {
    let mock_server = MockServer::start().await;

    // AutoMkcol 404
    Mock::given(method("PUT"))
        .and(path("/general/workspace/report.txt"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&mock_server)
        .await;

    // ensure_directory_all: /general (already exists → 405 silenced)
    Mock::given(method("MKCOL"))
        .and(path("/general"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&mock_server)
        .await;

    // ensure_directory_all: /general/workspace (created)
    Mock::given(method("MKCOL"))
        .and(path("/general/workspace"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    // Retry plain PUT succeeds
    Mock::given(method("PUT"))
        .and(path("/general/workspace/report.txt"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "write", "room_id": "general", "path": "workspace/report.txt", "content": "report"}"#,
        )
        .await
        .unwrap();
    assert!(result.contains("bytes"));
}

#[tokio::test]
async fn test_webdav_write_fallback_inner_mkdir_already_exists() {
    let mock_server = MockServer::start().await;

    // AutoMkcol 404
    Mock::given(method("PUT"))
        .and(path("/general/workspace/report.txt"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    // Both dir segments already exist → 405 for each
    Mock::given(method("MKCOL"))
        .and(path("/general"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&mock_server)
        .await;
    Mock::given(method("MKCOL"))
        .and(path("/general/workspace"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&mock_server)
        .await;

    // Retry plain PUT succeeds
    Mock::given(method("PUT"))
        .and(path("/general/workspace/report.txt"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "write", "room_id": "general", "path": "workspace/report.txt", "content": "ok"}"#,
        )
        .await
        .unwrap();
    assert!(result.contains("bytes"));
}

#[tokio::test]
async fn test_webdav_write_fallback_both_fail() {
    let mock_server = MockServer::start().await;

    // AutoMkcol 404
    Mock::given(method("PUT"))
        .and(path("/general/notes.txt"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    // ensure_directory_all succeeds
    Mock::given(method("MKCOL"))
        .and(path("/general"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    // Retry plain PUT also fails with non-404 (e.g. 403 forbidden)
    Mock::given(method("PUT"))
        .and(path("/general/notes.txt"))
        .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "write", "room_id": "general", "path": "notes.txt", "content": "hello"}"#,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("WebDAV write failed"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn test_webdav_write_fallback_non_404_error_propagates() {
    let mock_server = MockServer::start().await;

    // AutoMkcol fails with 401 — should propagate, not trigger fallback
    Mock::given(method("PUT"))
        .and(path("/general/notes.txt"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "write", "room_id": "general", "path": "notes.txt", "content": "hello"}"#,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("WebDAV write failed"),
        "unexpected error: {err}"
    );
}

// ─── WebDAV Ensure Room Directory Tests ────────────────────────────────────────

#[tokio::test]
async fn test_webdav_ensure_room_directory_creates() {
    let mock_server = MockServer::start().await;

    // ensure_directory_all for /general/
    Mock::given(method("MKCOL"))
        .and(path("/general"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    client.ensure_room_directory("general").await.unwrap();
}

#[tokio::test]
async fn test_webdav_ensure_room_directory_already_exists() {
    let mock_server = MockServer::start().await;

    // /general/ already exists → 405, silently ignored by ensure_directory_all
    Mock::given(method("MKCOL"))
        .and(path("/general"))
        .respond_with(ResponseTemplate::new(405))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    client.ensure_room_directory("general").await.unwrap();
}

#[tokio::test]
async fn test_webdav_write_first_time_in_room() {
    let mock_server = MockServer::start().await;

    // Step 1: ensure_room_directory for "general" — creates /general
    Mock::given(method("MKCOL"))
        .and(path("/general"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Step 2: write_file_with_fallback → auto_mkcol fails 404 (parent exists now but just simulating)
    // Actually with new code, we'd ensure_room_directory first, then write via fallback.
    // The write_file_with_fallback tries auto_mkcol first, which would work if ensuring was done.
    // Let's simulate the full "first time write" flow:

    // AutoMkcol write file (this would be called by the tool after ensure_room_directory)
    Mock::given(method("PUT"))
        .and(path("/general/notes.txt"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());

    // Ensure room dir first
    client.ensure_room_directory("general").await.unwrap();

    // Then write
    client
        .write_file_with_fallback("/general/notes.txt", "hello".as_bytes().to_vec())
        .await
        .unwrap();
}

// ─── Mock HTTP Tests: WebDavTool edit ────────────────────────────────────────

#[tokio::test]
async fn test_webdav_edit_success() {
    let mock_server = MockServer::start().await;
    let file_content = "# Title\n\nHello, world!\n\n## Section\n\nMore text.";

    // Step 1: read_file for edit — GET
    Mock::given(method("GET"))
        .and(path("/general/notes.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(file_content))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Step 2: write_file_with_fallback after edit — PUT with AutoMkcol
    Mock::given(method("PUT"))
        .and(path("/general/notes.md"))
        .and(header("X-NC-WebDAV-AutoMkcol", "1"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "edit", "room_id": "general", "path": "notes.md",
               "oldString": "Hello, world!",
               "newString": "Hello, universe!"}"#,
        )
        .await
        .unwrap();
    assert!(result.contains("Edited"));
    assert!(result.contains("notes.md"));
    assert!(result.contains("replaced 1 occurrence"));
}

#[tokio::test]
async fn test_webdav_edit_oldstring_not_found() {
    let mock_server = MockServer::start().await;
    let file_content = "# Title\n\nHello, world!";

    Mock::given(method("GET"))
        .and(path("/general/notes.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(file_content))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "edit", "room_id": "general", "path": "notes.md",
               "oldString": "This text is not in the file",
               "newString": "replacement"}"#,
        )
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("oldString not found"));
}

#[tokio::test]
async fn test_webdav_edit_multiple_matches() {
    let mock_server = MockServer::start().await;
    let file_content = "The cat sat on the mat. The cat is happy.";

    Mock::given(method("GET"))
        .and(path("/general/notes.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(file_content))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = make_test_client(&mock_server.uri());
    let tool = rockbot::tools::WebDavTool::new(client);

    let result = tool
        .execute(
            r#"{"action": "edit", "room_id": "general", "path": "notes.md",
               "oldString": "The cat",
               "newString": "A dog"}"#,
        )
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("found 2 times"));
}

// ─── Mock HTTP Tests: OpenRouterImageProvider.generate_image() ─────────────────

fn make_openrouter_image_config(mock_uri: &str) -> ProviderConfig {
    ProviderConfig {
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-or-v1-test".into(),
        base_url: ConfigUrl::try_new(mock_uri.to_string()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    }
}

#[tokio::test]
async fn test_openrouter_image_gen_success() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("Authorization", "Bearer sk-or-v1-test"))
        .and(header("Content-Type", "application/json"))
        .and(body_string_contains("\"modalities\":[\"image\"]"))
        .and(body_string_contains("a sunset"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "gen-abc123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Here is an image.",
                    "images": [{
                        "type": "image_url",
                        "image_url": { "url": "data:image/png;base64,iVBORw0KGgo=" }
                    }]
                },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 1, "total_tokens": 11 }
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = make_openrouter_image_config(&base);
    let provider = OpenRouterImageProvider::new(&config, "google/gemini-3.1-flash-image-preview").unwrap();
    let bytes = provider.generate_image(&ImageGenParams::new("a sunset")).await.unwrap();
    assert!(!bytes.is_empty());
}

#[tokio::test]
async fn test_openrouter_image_gen_with_img2img() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_string_contains("\"type\":\"image_url\""))
        .and(body_string_contains("https://example.com/photo.png"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "images": [{
                        "image_url": { "url": "data:image/png;base64,AAAA" }
                    }]
                }
            }]
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = make_openrouter_image_config(&base);
    let provider = OpenRouterImageProvider::new(&config, "google/gemini-3.1-flash-image-preview").unwrap();
    let mut params = ImageGenParams::new("edit this photo");
    params.image_urls = Some(vec!["https://example.com/photo.png".into()]);
    let bytes = provider.generate_image(&params).await.unwrap();
    assert!(!bytes.is_empty());
}

#[tokio::test]
async fn test_openrouter_image_gen_unauthorized() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": { "message": "Invalid API key" }
            })),
        )
        .mount(&mock_server)
        .await;

    let config = make_openrouter_image_config(&mock_server.uri());
    let provider = OpenRouterImageProvider::new(&config, "google/gemini-3.1-flash-image-preview").unwrap();
    let result = provider.generate_image(&ImageGenParams::new("test")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid API key"));
}

#[tokio::test]
async fn test_openrouter_image_gen_missing_images_field() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": { "content": "No images returned" }
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = make_openrouter_image_config(&mock_server.uri());
    let provider = OpenRouterImageProvider::new(&config, "google/gemini-3.1-flash-image-preview").unwrap();
    let result = provider.generate_image(&ImageGenParams::new("test")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no images"));
}

#[tokio::test]
async fn test_openrouter_image_gen_with_aspect_ratio() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_string_contains("\"aspect_ratio\":\"16:9\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "images": [{
                        "image_url": { "url": "data:image/png;base64,AAAA" }
                    }]
                }
            }]
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = make_openrouter_image_config(&base);
    let provider = OpenRouterImageProvider::new(&config, "google/gemini-3.1-flash-image-preview").unwrap();
    let mut params = ImageGenParams::new("a sunset");
    params.image_size = Some(rockbot::ImageSizeValue::Preset("landscape_16_9".into()));
    params.quality = Some("high".into());
    params.output_format = Some("webp".into());
    params.num_images = Some(2);
    let bytes = provider.generate_image(&params).await.unwrap();
    assert!(!bytes.is_empty());
}

#[tokio::test]
async fn test_openrouter_upload_file_data_uri() {
    let config = make_openrouter_image_config("https://openrouter.ai/api/v1");
    let provider = OpenRouterImageProvider::new(&config, "google/gemini-3.1-flash-image-preview").unwrap();
    let result = provider.upload_file(b"fake-png", "image/png").await.unwrap();
    assert_eq!(result, "data:image/png;base64,ZmFrZS1wbmc=");
}

// ─── Mock HTTP Tests: FalAiProvider.generate_image_url() ──────────────────────────

fn make_fal_config(mock_uri: &str) -> ProviderConfig {
    ProviderConfig {
        name: ProviderName::try_new("fal".to_string()).unwrap(),
        api_key: "fal-test-key".into(),
        base_url: ConfigUrl::try_new(mock_uri.to_string()).unwrap(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models: HashMap::new(),
    }
}

#[tokio::test]
async fn test_fal_submit_request() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .and(header("Authorization", "Key fal-test-key"))
        .and(header("Content-Type", "application/json"))
        .and(body_string_contains("\"prompt\":\"a sunset\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-abc-123",
            "status_url": format!("{}/fal-ai/flux/schnell/requests/req-abc-123/status", base),
            "response_url": format!("{}/fal-ai/flux/schnell/requests/req-abc-123", base),
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fal-ai/flux/schnell/requests/req-abc-123/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "COMPLETED"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fal-ai/flux/schnell/requests/req-abc-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "images": [{"url": "https://fal.media/result.png", "width": 1024, "height": 1024}]
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&base);
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    let url = provider.generate_image_url(&ImageGenParams::new("a sunset")).await.unwrap();
    assert_eq!(url, "https://fal.media/result.png");
}

#[tokio::test]
async fn test_fal_submit_body_includes_image_size_when_aspect_ratio_preset() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .and(header("Authorization", "Key fal-test-key"))
        .and(body_string_contains("\"image_size\""))
        .and(body_string_contains("\"width\":3840"))
        .and(body_string_contains("\"height\":2160"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-ar-16x9",
            "status_url": format!("{base}/fal-ai/flux/schnell/requests/req-ar-16x9/status"),
            "response_url": format!("{base}/fal-ai/flux/schnell/requests/req-ar-16x9"),
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fal-ai/flux/schnell/requests/req-ar-16x9/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "COMPLETED"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fal-ai/flux/schnell/requests/req-ar-16x9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "images": [{"url": "https://fal.media/ar-16x9.png", "width": 3840, "height": 2160}]
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&base);
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    let mut params = ImageGenParams::new("a sunset");
    params.image_size = Some(rockbot::types::ImageSizeValue::Preset("16:9".into()));
    let url = provider.generate_image_url(&params).await.unwrap();
    assert_eq!(url, "https://fal.media/ar-16x9.png");
}

#[tokio::test]
async fn test_fal_submit_body_omits_image_size_when_none() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .and(header("Authorization", "Key fal-test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-no-size",
            "status_url": format!("{base}/fal-ai/flux/schnell/requests/req-no-size/status"),
            "response_url": format!("{base}/fal-ai/flux/schnell/requests/req-no-size"),
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fal-ai/flux/schnell/requests/req-no-size/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "COMPLETED"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fal-ai/flux/schnell/requests/req-no-size"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "images": [{"url": "https://fal.media/no-size.png", "width": 1024, "height": 1024}]
        })))
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&base);
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    // No image_size set — params default to None
    let url = provider.generate_image_url(&ImageGenParams::new("a sunset")).await.unwrap();
    assert_eq!(url, "https://fal.media/no-size.png");
}

#[tokio::test]
async fn test_fal_submit_unauthorized() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(serde_json::json!({"detail": "Invalid key"})),
        )
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&mock_server.uri());
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    let result = provider.generate_image_url(&ImageGenParams::new("test")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid key"));
}

#[tokio::test]
async fn test_fal_submit_missing_request_id() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&mock_server.uri());
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    let result = provider.generate_image_url(&ImageGenParams::new("test")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fal_poll_status_failed() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-fail-1",
            "status_url": format!("{}/fal-ai/flux/schnell/requests/req-fail-1/status", base),
            "response_url": format!("{}/fal-ai/flux/schnell/requests/req-fail-1", base),
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fal-ai/flux/schnell/requests/req-fail-1/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "FAILED",
            "error": "NSFW content detected"
        })))
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&base);
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    let result = provider.generate_image_url(&ImageGenParams::new("test")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("NSFW"));
}

#[tokio::test]
async fn test_fal_poll_status_http_error() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-err-1",
            "status_url": format!("{}/fal-ai/flux/schnell/requests/req-err-1/status", base),
            "response_url": format!("{}/fal-ai/flux/schnell/requests/req-err-1", base),
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fal-ai/flux/schnell/requests/req-err-1/status"))
        .respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({
            "detail": "Service temporarily unavailable"
        })))
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&base);
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    let result = provider.generate_image_url(&ImageGenParams::new("test")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("503"));
}

#[tokio::test]
async fn test_fal_submit_missing_status_url() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-no-status",
            "response_url": "/fal-ai/flux/schnell/requests/req-no-status"
        })))
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&mock_server.uri());
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    let result = provider.generate_image_url(&ImageGenParams::new("test")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("status_url"));
}

#[tokio::test]
async fn test_fal_submit_missing_response_url() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-no-resp",
            "status_url": format!("{}/fal-ai/flux/schnell/requests/req-no-resp/status", base),
        })))
        .mount(&mock_server)
        .await;

    let config = make_fal_config(&base);
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    let result = provider.generate_image_url(&ImageGenParams::new("test")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("response_url"));
}

// ─── WebDavTool: webdav_dir schema test ─────────────────────────────────────

#[test]
fn test_webdav_tool_webdav_dir_not_in_llm_schema() {
    let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
    let tool = rockbot::tools::WebDavTool::new(client);
    let params = tool.parameters();
    assert!(
        params["properties"].get("webdav_dir").is_none(),
        "webdav_dir should not be in LLM-facing schema (injected by harness)"
    );
}

// ─── Memory Conflict: Multiple Messages Before Bot Response ─────────────────

/// Simulates rapid incoming messages before bot responds.
/// Verifies memory cache is not corrupted and all messages are preserved.
#[test]
fn test_memory_rapid_messages_no_loss() {
    let mut mm = rockbot::memory::MemoryManager::new(
        2000,  // max_soul_chars
        60,    // persist_interval_secs
        0,     // max_context_bytes (disabled)
    );

    let room_id = "test-room-rapid";
    // Create room state
    let _room = mm.get_or_create(room_id, "test-channel", "Test Channel", false);

    // Simulate 5 rapid incoming messages before bot responds
    let messages = [
        ("alice", "Hello bot!"),
        ("bob", "@rockbot help"),
        ("charlie", "What's the weather?"),
        ("alice", "Also tell me about Docker"),
        ("bob", "@rockbot urgent: server down"),
    ];

    for (sender, text) in &messages {
        let msg = ChatMessage::user(format!("{}: {}", sender, text));
        if let Some(room) = mm.get_mut(room_id) {
            room.history.append(msg);
        }
    }

    // Verify all messages are present
    let room = mm.get(room_id).unwrap();
    assert_eq!(
        room.history.messages.len(),
        5,
        "All 5 rapid messages should be in history"
    );

    // Verify message content is preserved
    for (i, (sender, text)) in messages.iter().enumerate() {
        let msg = &room.history.messages[i];
        let msg_text = format!("{:?}", msg.content);
        assert!(
            msg_text.contains(sender) && msg_text.contains(text),
            "Message {} content should match: got '{}'",
            i,
            msg_text
        );
    }
}

/// Verifies that snapshot loading (history + soul)
/// doesn't conflict with in-memory state when both are loaded together.
#[test]
fn test_memory_load_snapshot_with_soul_no_conflict() {
    let mut mm = rockbot::memory::MemoryManager::new(
        2000, 60, 0,
    );

    let room_id = "snapshot-test";

    // Pre-populate soul
    let soul = rockbot::memory::SoulMemory {
        room_id: NonEmptyString::try_new(room_id.to_string()).unwrap(),
        content: "# Soul Memory\n\n- My name is TestBot\n- likes Rust".to_string(),
        updated_at: "2026-06-10T00:00:00Z".to_string(),
    };
    mm.set_soul(room_id, soul);

    // Add messages to history
    let room = mm.get_or_create(room_id, "snaproom", "SnapRoom", false);
    room.history.append(ChatMessage::user("alice: Hello"));
    room.history.append(ChatMessage::assistant("Hi Alice!"));
    mm.mark_snapshot_dirty(room_id);

    // Build snapshot
    let snap = mm.build_snapshot(room_id);
    assert!(snap.is_some(), "Should build snapshot");
    let snap = snap.unwrap();
    assert_eq!(snap.messages.len(), 2, "Snapshot should have both messages");
    assert_eq!(snap.soul.as_deref(), Some("# Soul Memory\n\n- My name is TestBot\n- likes Rust"));

    // Verify all data is consistent (soul, history coexist)
    let ctx = mm.build_context(room_id, "You are a helpful bot.", None, None);
    assert!(!ctx.is_empty(), "Context should not be empty");

    // Verify soul is in context (as system message)
    let has_soul = ctx.iter().any(|m| {
        format!("{:?}", m.content).contains("TestBot")
            && format!("{:?}", m.content).contains("likes Rust")
    });
    assert!(has_soul, "Context should include soul content");
}

/// Tests that concurrent snapshot builds and memory mutations
/// don't lose data. Simulates: messages arrive -> build snapshot ->
/// messages arrive again -> build snapshot again.
#[test]
fn test_memory_snapshot_repeated_builds_no_data_loss() {
    let mut mm = rockbot::memory::MemoryManager::new(
        2000, 60, 0,
    );

    let room_id = "repeated-snap";
    let _room = mm.get_or_create(room_id, "repsnap", "RepSnap", false);

    // Batch 1: 3 messages
    for (sender, text) in [("alice", "msg1"), ("bob", "msg2"), ("charlie", "msg3")] {
        let msg = ChatMessage::user(format!("{}: {}", sender, text));
        if let Some(room) = mm.get_mut(room_id) {
            room.history.append(msg);
        }
        mm.mark_snapshot_dirty(room_id);
    }

    let snap1 = mm.build_snapshot(room_id);
    assert!(snap1.is_some());
    assert_eq!(snap1.as_ref().unwrap().messages.len(), 3);

    // Batch 2: 2 more messages
    for (sender, text) in &[("alice", "msg4"), ("bob", "msg5")] {
        let msg = ChatMessage::user(format!("{}: {}", sender, text));
        if let Some(room) = mm.get_mut(room_id) {
            room.history.append(msg);
        }
        mm.mark_snapshot_dirty(room_id);
    }

    let snap2 = mm.build_snapshot(room_id);
    assert!(snap2.is_some());
    assert_eq!(snap2.as_ref().unwrap().messages.len(), 5, "All 5 messages should be in snapshot");

    // Verify no messages lost across snapshots
    let all_texts: Vec<String> = snap2
        .unwrap()
        .messages
        .iter()
        .map(|m| format!("{:?}", m.content))
        .collect();

    for expected in &["msg1", "msg2", "msg3", "msg4", "msg5"] {
        assert!(
            all_texts.iter().any(|t| t.contains(expected)),
            "Expected message containing '{}' not found in snapshot",
            expected
        );
    }
}

/// Tests memory TTL eviction doesn't lose un-persisted messages
// when multiple rooms are active simultaneously
#[test]
fn test_memory_multi_room_no_cross_contamination() {
    let mut mm = rockbot::memory::MemoryManager::new(
        2000, 60, 0,
    );

    let room1 = mm.get_or_create("r1", "channel-a", "Channel A", false);
    room1.history.append(ChatMessage::user("alice: room1 msg1"));
    room1.history.append(ChatMessage::assistant("bot: room1 reply"));

    let room2 = mm.get_or_create("r2", "dm-bob", "DM Bob", true);
    room2.history.append(ChatMessage::user("bob: room2 msg1"));

    // Set soul in room1
    mm.set_soul("r1", rockbot::memory::SoulMemory {
        room_id: NonEmptyString::try_new("r1".to_string()).unwrap(),
        content: "# Soul Memory\n\n- My name is Room1Bot".to_string(),
        updated_at: String::new(),
    });

    // Set knowledge in room2
    mm.set_knowledge("r2", "[Knowledge]\n- Room2 daily notes".to_string());

    // Build context for each room - should not cross-contaminate
    let ctx1 = mm.build_context("r1", "You are a bot.", None, None);
    let ctx2 = mm.build_context("r2", "You are a bot.", None, None);

    // Room1 has soul but not room2's knowledge
    let room1_has_own_soul = ctx1.iter().any(|m| {
        format!("{:?}", m.content).contains("Room1Bot")
    });
    assert!(room1_has_own_soul, "Room1 context should include its own soul");

    let room1_has_room2_data = ctx1.iter().any(|m| {
        format!("{:?}", m.content).contains("Room2 daily")
    });
    assert!(!room1_has_room2_data, "Room1 context should NOT include Room2's knowledge");

    // Room2 has knowledge but not room1's soul
    let room2_has_own_knowledge = ctx2.iter().any(|m| {
        format!("{:?}", m.content).contains("Room2 daily")
    });
    assert!(room2_has_own_knowledge, "Room2 context should include its own knowledge");

    let room2_has_room1_data = ctx2.iter().any(|m| {
        format!("{:?}", m.content).contains("Room1Bot")
    });
    assert!(!room2_has_room1_data, "Room2 context should NOT include Room1's soul");
}

// ─── NextCloud Share Link structural test (image-gen.md §3) ─────────────────

#[tokio::test]
async fn test_nextcloud_share_link_compiles_and_handles_no_server() {
    // Verifies create_nextcloud_share_link exists, compiles with correct signatures,
    // and returns None gracefully when no server is available (doesn't panic).
    // Full wiremock coverage requires fixing the port-number-in-server-root bug
    // where url::Url::port() is dropped during scheme+host extraction.
    let client = webdav::WebDavClient::new(
        "https://cloud.example.com/remote.php/dav/files/user/rockbot",
        "testuser",
        "testpass",
    )
    .unwrap();
    let result = client.create_nextcloud_share_link("images/test.png").await;
    assert!(result.is_none(), "No server available → None");
}

// ─── Mock HTTP Tests: LlamaCppProvider.complete() ────────────────────────────

#[tokio::test]
async fn test_llamacpp_complete_passes_image_through_to_server() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("\"type\":\"image_url\""))
        .and(body_string_contains("data:image/png;base64,iVBOR"))
        .and(body_string_contains("\"type\":\"text\""))
        .and(body_string_contains("describe this"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-local",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "I see a red apple on a wooden table."
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 120,
                "completion_tokens": 10,
                "total_tokens": 130
            }
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("llamacpp".to_string()).unwrap(),
        api_key: "".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/v1/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider =
        rockbot::provider::LlamaCppProvider::new(&config, "local-model").unwrap();

    let request = ChatRequest {
        model: "local-model".into(),
        messages: vec![ChatMessage::user_with_image(
            "describe this",
            "data:image/png;base64,iVBOR",
        )],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(
        result.text,
        Some("I see a red apple on a wooden table.".into())
    );
    assert_eq!(result.finish, FinishReason::Stop);
    assert!(result.tool_calls.is_empty());
}

// ─── Mock HTTP Tests: LlamaCppProvider system-message coalesce (issue #77) ────

/// Custom matcher asserting the request body carries exactly ONE system
/// message and that it sits at index 0 (strict chat templates, e.g.
/// Qwen3.5/3.6-derived Bonsai-27B, reject any system message at index >= 1).
struct SingleLeadingSystemMessage;

impl Match for SingleLeadingSystemMessage {
    fn matches(&self, request: &Request) -> bool {
        let body: serde_json::Value = match serde_json::from_slice(&request.body) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let messages = match body.get("messages").and_then(|m| m.as_array()) {
            Some(arr) => arr,
            None => return false,
        };
        let system_count = messages
            .iter()
            .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
            .count();
        system_count == 1
            && messages
                .first()
                .and_then(|m| m.get("role"))
                .and_then(|r| r.as_str())
                == Some("system")
    }
}

/// Gitea #77 happy path: a rockbot-style request (system prompt + soul +
/// user) must arrive at the llama.cpp server with the leading system
/// messages coalesced into a single system message at index 0.
#[tokio::test]
async fn test_llamacpp_complete_coalesces_leading_system_messages() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(SingleLeadingSystemMessage)
        .and(body_string_contains("You are helpful"))
        .and(body_string_contains("My name is TestBot"))
        .and(body_string_contains("hello"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-local",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hi there!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 50,
                "completion_tokens": 5,
                "total_tokens": 55
            }
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("llamacpp".to_string()).unwrap(),
        api_key: "".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/v1/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = LlamaCppProvider::new(&config, "local-model").unwrap();

    let request = ChatRequest {
        model: "local-model".into(),
        messages: vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::system("[Core memory]\n- My name is TestBot"),
            ChatMessage::user("hello"),
        ],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("Hi there!".into()));
    assert_eq!(result.finish, FinishReason::Stop);
}

// ─── Mock HTTP Tests: Memory Compression Pipeline ─────────────────────────────

use rockbot::harness::AgentHarness;

#[cfg(test)]
mod summarization_tests {
    use super::*;
    use rockbot::types::CompletionResult;
    use rockbot::validated::BoundedUsize;

    /// Mock AI provider that returns text and tracks whether it was called.
    struct CountingMockProvider {
        text: String,
        call_count: std::sync::atomic::AtomicUsize,
    }

    impl CountingMockProvider {
        fn new(text: &str) -> Self {
            Self {
                text: text.to_string(),
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl AiProvider for CountingMockProvider {
        async fn complete(&self, _request: ChatRequest) -> Result<CompletionResult, RockBotError> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(CompletionResult {
                text: Some(self.text.clone()),
                tool_calls: vec![],
                finish: FinishReason::Stop,
                reasoning_content: None,
                usage: Some(rockbot::types::UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 10,
                    total_tokens: 10,
                }),
            })
        }

        fn provider_name(&self) -> &str {
            "counting-mock"
        }

        fn model_name(&self) -> &str {
            "counting-model"
        }
    }

    /// Mock AI provider that always returns an error.
    struct FailingMockProvider;

    #[async_trait::async_trait]
    impl AiProvider for FailingMockProvider {
        async fn complete(&self, _request: ChatRequest) -> Result<CompletionResult, RockBotError> {
            Err(RockBotError::Provider("mock provider failure".into()))
        }

        fn provider_name(&self) -> &str {
            "failing-mock"
        }

        fn model_name(&self) -> &str {
            "failing-model"
        }
    }

    fn make_summarization_test_config() -> rockbot::config::AppConfig {
        make_summarization_test_config_with(true)
    }

    fn make_summarization_test_config_with(summarization_enabled: bool) -> rockbot::config::AppConfig {
        use rockbot::config::{ImageModelConfig, ModelConfig, RocketChatSection, ServerConfig};
        rockbot::config::AppConfig {
            platform: Default::default(),
            rocketchat: RocketChatSection {
                server: ServerConfig {
                    url: "test.example.com".into(),
                    username: "bot".into(),
                    password: "secret".into(),
                    debug: false,
                },
                model: None,
            },
            matrix: None,
            model: ModelConfig {
                default_provider: ProviderName::try_new("counting-mock".to_string()).unwrap(),
                default_model: "counting-model".into(),
                max_iterations: 5,
                max_soul_chars: BoundedUsize::try_new(5000).unwrap(),
                persist_interval_secs: 60,
                memory_ttl_secs: 86400,
                max_context_bytes: BoundedUsize::try_new(4194304).unwrap(),
                max_attachment_bytes: 20971520,
                model_context_length: 1_000_000,
                summarization_enabled,
                summarization_ratio: 0.6,
                summarization_target_tokens: 1024,
            },
            chat_providers: vec![rockbot::config::ProviderConfig {
                name: ProviderName::try_new("counting-mock".to_string()).unwrap(),
                api_key: "sk-test".into(),
                base_url: ConfigUrl::try_new("https://mock.ai/v1".to_string()).unwrap(),
                basecf_url: None,
                chat_path: Some("/chat/completions".into()),
                draw_path: None,
                models: HashMap::new(),
            }],
            image_providers: vec![],
            image_model: ImageModelConfig {
                default_provider: ProviderName::try_new("mock".to_string()).unwrap(),
                default_text_model: "mock-img".into(),
                default_edit_model: "mock-img-edit".into(),
                default_quality: "standard".into(),
                default_output_format: "png".into(),
                default_num_images: 1,
                default_image_size: "1024x1024".into(),
                default_image_size_tier: "1K".into(),
                default_enable_safety_checker: false,
            },
            tools: HashMap::new(),
            search: Default::default(),
            webdav: None,
            agent: Default::default(),
            acp: None,
        }
    }

    /// Token pressure flag triggers LLM summarization instead of full wipe.
    /// 20 messages → oldest 12 (60%) summarized into 1 system msg, 8 retained.
    #[tokio::test]
    async fn test_token_pressure_triggers_summarization() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let config = make_summarization_test_config();
        let provider = Box::new(CountingMockProvider::new("Summary of conversation"));
        let mut harness = AgentHarness::new(config, provider, None, image_cache, "@testbot");

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..20 {
            room.history.append(ChatMessage::user(format!(
                "Message number {} with some extra padding text for testing",
                i
            )));
        }
        assert_eq!(room.history.messages.len(), 20);

        harness.memory_mut().set_token_pressure("room1");

        harness.reset_room_if_needed("room1").await.unwrap();

        let room = harness.memory().get("room1").unwrap();
        let after = room.history.messages.len();
        // 20 - 12 (summarized) + 1 (summary msg) = 9
        assert_eq!(after, 9, "Should have 1 summary + 8 recent messages");

        // First message should be the summary system message
        assert_eq!(room.history.messages[0].role, rockbot::types::Role::System);
        let summary_text = room.history.messages[0].text_content().unwrap();
        assert!(summary_text.contains("Summary of conversation"));
        assert!(summary_text.contains("[Conversation Summary"));

        // Remaining messages should be the original recent ones
        assert_eq!(room.history.messages[1].role, rockbot::types::Role::User);
        assert!(room.history.messages[1].text_content().unwrap().contains("Message number 12"));
    }

    /// Byte pressure flag triggers LLM summarization.
    /// 15 messages → oldest 9 (60%) summarized into 1 system msg, 6 retained.
    #[tokio::test]
    async fn test_byte_pressure_triggers_summarization() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let config = make_summarization_test_config();
        let provider = Box::new(CountingMockProvider::new("Byte pressure summary"));
        let mut harness = AgentHarness::new(config, provider, None, image_cache, "@testbot");

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..15 {
            room.history.append(ChatMessage::user(format!(
                "Repeating message number {} with ample padding",
                i
            )));
        }
        assert_eq!(room.history.messages.len(), 15);

        harness.memory_mut().set_byte_pressure("room1");

        harness.reset_room_if_needed("room1").await.unwrap();

        let room = harness.memory().get("room1").unwrap();
        let after = room.history.messages.len();
        // 15 - 9 (summarized) + 1 (summary msg) = 7
        assert_eq!(after, 7, "Should have 1 summary + 6 recent messages");

        assert_eq!(room.history.messages[0].role, rockbot::types::Role::System);
        assert!(room.history.messages[0].text_content().unwrap().contains("Byte pressure summary"));
    }

    /// When LLM summarization fails, falls back to strip-half (drop oldest 50%).
    #[tokio::test]
    async fn test_summarization_llm_failure_falls_back_to_strip_half() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let config = make_summarization_test_config();
        let provider = Box::new(FailingMockProvider);
        let mut harness = AgentHarness::new(config, provider, None, image_cache, "@testbot");

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..20 {
            room.history.append(ChatMessage::user(format!("Message {}", i)));
        }
        assert_eq!(room.history.messages.len(), 20);

        harness.memory_mut().set_token_pressure("room1");

        harness.reset_room_if_needed("room1").await.unwrap();

        let room = harness.memory().get("room1").unwrap();
        let after = room.history.messages.len();
        // Strip half: 20 / 2 = 10 removed, 10 remain
        assert_eq!(after, 10, "Should strip half (10 of 20 messages)");

        // No summary system message should be present
        assert_ne!(room.history.messages[0].role, rockbot::types::Role::System);
    }

    /// Explicit reset still does full wipe (all messages cleared).
    #[tokio::test]
    async fn test_explicit_reset_still_does_full_wipe() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let config = make_summarization_test_config();
        let provider = Box::new(CountingMockProvider::new("should not be called"));
        let mut harness = AgentHarness::new(config, provider, None, image_cache, "@testbot");

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..10 {
            room.history.append(ChatMessage::user(format!("Message {}", i)));
        }
        assert_eq!(room.history.messages.len(), 10);

        harness.memory_mut().set_explicit_reset("room1");

        let result = harness.reset_room_if_needed("room1").await.unwrap();
        assert!(result.did_reset);
        assert!(result.was_explicit);
        assert_eq!(result.messages_cleared, 10);

        let after = harness
            .memory()
            .get("room1")
            .map(|r| r.history.messages.len())
            .unwrap_or(0);
        assert_eq!(after, 0, "All messages should be cleared on explicit reset");
    }

    /// When summarization is disabled, pressure triggers strip-half, not full wipe.
    #[tokio::test]
    async fn test_summarization_disabled_strips_half() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let config = make_summarization_test_config_with(false);
        let provider = Box::new(CountingMockProvider::new("should not be called"));
        let mut harness = AgentHarness::new(config, provider, None, image_cache, "@testbot");

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..20 {
            room.history.append(ChatMessage::user(format!("Message {}", i)));
        }
        assert_eq!(room.history.messages.len(), 20);

        harness.memory_mut().set_token_pressure("room1");

        harness.reset_room_if_needed("room1").await.unwrap();

        let room = harness.memory().get("room1").unwrap();
        let after = room.history.messages.len();
        // Strip half: 20 / 2 = 10 removed, 10 remain
        assert_eq!(after, 10, "Should strip half when summarization disabled");

        // No summary system message
        assert_ne!(room.history.messages[0].role, rockbot::types::Role::System);
    }

    /// No pressure flags → no change to history.
    #[tokio::test]
    async fn test_no_pressure_no_change() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let config = make_summarization_test_config();
        let provider = Box::new(CountingMockProvider::new("ok"));
        let mut harness = AgentHarness::new(config, provider, None, image_cache, "@testbot");

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..20 {
            room.history.append(ChatMessage::user(format!("msg {}", i)));
        }
        let before = room.history.messages.len();

        harness.reset_room_if_needed("room1").await.unwrap();

        let after = harness
            .memory()
            .get("room1")
            .map(|r| r.history.messages.len())
            .unwrap_or(0);
        assert_eq!(after, before, "No flags → no change, all messages remain");
    }

    /// Pressure flags are cleared after summarization completes.
    #[tokio::test]
    async fn test_pressure_flags_cleared_after_summarization() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let config = make_summarization_test_config();
        let provider = Box::new(CountingMockProvider::new("summary"));
        let mut harness = AgentHarness::new(config, provider, None, image_cache, "@testbot");

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..10 {
            room.history.append(ChatMessage::user(format!("msg {}", i)));
        }

        harness.memory_mut().set_token_pressure("room1");
        assert!(harness.memory().has_token_pressure("room1"));

        harness.reset_room_if_needed("room1").await.unwrap();

        assert!(!harness.memory().has_token_pressure("room1"), "Token pressure should be cleared");
        assert!(!harness.memory().has_byte_pressure("room1"), "Byte pressure should be cleared");
        assert!(!harness.memory().has_explicit_reset("room1"), "Explicit reset should be cleared");
    }
}

// ─── _dfd/knowledge/knowledge.md — Knowledge Cache TTL (happy path) ────────────

#[cfg(test)]
mod knowledge_cache_tests {
    use super::*;
    use rockbot::AgentHarness;
    use rockbot::config::{AppConfig, ImageModelConfig, ModelConfig, RocketChatSection, ServerConfig};
    use rockbot::validated::{BoundedUsize, ConfigUrl, ProviderName};
    use wiremock::matchers::{method, path};

    fn make_knowledge_test_config(base_url: &str) -> AppConfig {
        let webdav_cfg = {
            let toml_str = format!(
                r#"
url = "{base_url}"
username = "testuser"
password = "testpass"
root = "rockbot"
dav_path = "remote.php/dav"
"#
            );
            Some(toml::from_str(&toml_str).expect("valid WebDavConfig TOML"))
        };

        AppConfig {
            platform: Default::default(),
            rocketchat: RocketChatSection {
                server: ServerConfig {
                    url: "test.example.com".into(),
                    username: "bot".into(),
                    password: "secret".into(),
                    debug: false,
                },
                model: None,
            },
            matrix: None,
            model: ModelConfig {
                default_provider: ProviderName::try_new("mock".to_string()).unwrap(),
                default_model: "mock-model".into(),
                max_soul_chars: BoundedUsize::try_new(5000).unwrap(),
                max_iterations: 5,
                persist_interval_secs: 60,
                memory_ttl_secs: 86400,
                max_context_bytes: BoundedUsize::try_new(4194304).unwrap(),
                max_attachment_bytes: 20971520,
                model_context_length: 1_000_000,
                summarization_enabled: true,
                summarization_ratio: 0.6,
                summarization_target_tokens: 1024,
            },
            chat_providers: vec![rockbot::config::ProviderConfig {
                name: ProviderName::try_new("mock".to_string()).unwrap(),
                api_key: "sk-test".into(),
                base_url: ConfigUrl::try_new(format!("{}/v1", base_url)).unwrap(),
                basecf_url: None,
                chat_path: Some("/chat/completions".into()),
                draw_path: None,
                models: HashMap::new(),
            }],
            image_providers: vec![],
            image_model: ImageModelConfig {
                default_provider: ProviderName::try_new("mock".to_string()).unwrap(),
                default_text_model: "mock-img".into(),
                default_edit_model: "mock-img-edit".into(),
                default_quality: "standard".into(),
                default_output_format: "png".into(),
                default_num_images: 1,
                default_image_size: "1024x1024".into(),
                default_image_size_tier: "1K".into(),
                default_enable_safety_checker: false,
            },
            tools: HashMap::new(),
            search: Default::default(),
            webdav: webdav_cfg,
            agent: Default::default(),
            acp: None,
        }
    }

    struct MockMinProvider;
    #[async_trait::async_trait]
    impl AiProvider for MockMinProvider {
        async fn complete(&self, _req: ChatRequest) -> rockbot::error::Result<rockbot::types::CompletionResult> {
            Err(RockBotError::Provider("mock provider not for chat".into()))
        }
        fn provider_name(&self) -> &str { "mock-min" }
        fn model_name(&self) -> &str { "mock-min" }
    }

    #[tokio::test]
    async fn test_knowledge_index_summary_injected() {
        let mock_server = MockServer::start().await;
        let base_url = mock_server.uri();

        let index_json = serde_json::json!({
            "version": "rockbot-knowledge/1",
            "room_id": "r-summary",
            "entries": [
                {
                    "filename": "p0_critical.md",
                    "when_useful": "Always important",
                    "priority": "P0",
                    "last_promoted_at": null
                },
                {
                    "filename": "p1_common.md",
                    "when_useful": "When working with databases",
                    "priority": "P1",
                    "last_promoted_at": null
                },
                {
                    "filename": "p3_item.md",
                    "when_useful": "When working with APIs",
                    "priority": "P3",
                    "last_promoted_at": null
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path("/r-summary/knowledge/index.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&index_json))
            .expect(1)
            .mount(&mock_server)
            .await;

        // .md files should NOT be requested during context injection
        for f in &["p0_critical.md", "p1_common.md", "p3_item.md"] {
            Mock::given(method("GET"))
                .and(path(format!("/r-summary/knowledge/{}", f)))
                .respond_with(ResponseTemplate::new(200).set_body_string("should not be downloaded"))
                .expect(0)
                .mount(&mock_server)
                .await;
        }

        let config = make_knowledge_test_config(&base_url);
        let provider = Box::new(MockMinProvider);
        let webdav = webdav::WebDavClient::new(&base_url, "testuser", "testpass").unwrap();
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let mut harness = AgentHarness::new(config, provider, Some(webdav), image_cache, "@testbot");

        harness.memory_mut().get_or_create("room1", "summary", "", false)
            .history.append(ChatMessage::user("tell me about databases"));

        harness.refresh_knowledge_context("room1", "r-summary").await.unwrap();

        let knowledge = harness.memory().get_knowledge("room1");
        assert!(knowledge.is_some(), "knowledge should be injected");
        let text = knowledge.unwrap();
        assert!(text.contains("[Knowledge Index"), "should contain index header");
        assert!(text.contains("[P0] p0_critical"), "should list P0 entry with priority tag");
        assert!(text.contains("[P1] p1_common"), "should list P1 entry with priority tag");
        assert!(text.contains("[P3] p3_item"), "should list P3 entry with priority tag");
        assert!(text.contains("Always important"), "should include when_useful for P0");
        assert!(text.contains("When working with databases"), "should include when_useful for P1");
        assert!(text.contains("When working with APIs"), "should include when_useful for P3");
        assert!(!text.contains("should not be downloaded"), "should not contain .md body content");
    }

    #[tokio::test]
    async fn test_knowledge_stale_cleared_on_refresh() {
        let mock_server = MockServer::start().await;
        let base_url = mock_server.uri();

        let index_empty = serde_json::json!({
            "version": "rockbot-knowledge/1",
            "room_id": "r-stale",
            "entries": []
        });

        Mock::given(method("GET"))
            .and(path("/r-stale/knowledge/index.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&index_empty))
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = make_knowledge_test_config(&base_url);
        let provider = Box::new(MockMinProvider);
        let webdav = webdav::WebDavClient::new(&base_url, "testuser", "testpass").unwrap();
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let mut harness = AgentHarness::new(config, provider, Some(webdav), image_cache, "@testbot");

        harness.memory_mut().get_or_create("room1", "stale", "", false);

        // Simulate stale knowledge from a prior turn
        harness.memory_mut().set_knowledge("room1", "[Knowledge Index]\n[P1] old_note — something".to_string());
        assert!(!harness.memory().get_knowledge("room1").unwrap().is_empty(),
            "precondition: knowledge should be non-empty");

        // Refresh with empty index — should clear stale knowledge
        harness.refresh_knowledge_context("room1", "r-stale").await.unwrap();

        let knowledge = harness.memory().get_knowledge("room1");
        assert!(knowledge.is_some());
        assert!(knowledge.unwrap().is_empty(), "knowledge should be cleared after refresh with empty index");
    }
}

// ─── Mock HTTP Tests: LlamaCppProvider auth header (issue #73) ────────────────

#[tokio::test]
async fn test_llamacpp_sends_bearer_header_when_key_set() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("Authorization", "Bearer sk-local-key"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("llamacpp".to_string()).unwrap(),
        api_key: "sk-local-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = LlamaCppProvider::new(&config, "local-model").unwrap();

    let request = ChatRequest {
        model: "local-model".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("ok".into()));
}

#[tokio::test]
async fn test_llamacpp_omits_auth_header_when_key_empty() {
    let mock_server = MockServer::start().await;

    // Any Authorization header would make this matcher fail (header present
    // with any value is not allowed), so assert the request has NO auth header.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(NoAuthHeader)
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "no-auth" },
                "finish_reason": "stop"
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("llamacpp".to_string()).unwrap(),
        api_key: "".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = LlamaCppProvider::new(&config, "local-model").unwrap();

    let request = ChatRequest {
        model: "local-model".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let result = provider.complete(request).await.unwrap();
    assert_eq!(result.text, Some("no-auth".into()));
}

#[tokio::test]
async fn test_llamacpp_401_maps_to_auth_failed() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": { "message": "Invalid API Key", "type": "authenticationerror", "code": 401 }
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        name: ProviderName::try_new("llamacpp".to_string()).unwrap(),
        api_key: "wrong-key".into(),
        base_url: ConfigUrl::try_new(mock_server.uri()).unwrap(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = LlamaCppProvider::new(&config, "local-model").unwrap();

    let request = ChatRequest {
        model: "local-model".into(),
        messages: vec![ChatMessage::user("Hi")],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };

    let err = provider.complete(request).await.unwrap_err();
    assert!(matches!(err, RockBotError::AuthFailed(_)), "expected AuthFailed, got {err:?}");
}

// ─── Gitea #80 — Tool-Call JSON Parse Error Recovery ────────────────────────
//
// _dfd/agent/agent-harness.md §2k — the provider rejects a request because a
// tool call `arguments` JSON document failed to parse (e.g. an 11K-char
// truncated report argument — `[json.exception.parse_error.101] ... invalid
// string: missing closing quote`). The harness must repair tool call args in
// history and retry once before falling back to the error reply.

#[cfg(test)]
mod tool_call_parse_recovery_tests {
    use super::*;
    use rockbot::config::{ImageModelConfig, ModelConfig, RocketChatSection, ServerConfig};
    use rockbot::harness::AgentHarness;
    use rockbot::types::{CompletionResult, ToolCall};
    use rockbot::validated::BoundedUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    const NLOHMANN_PARSE_ERROR: &str = "Failed to parse tool call arguments as JSON:\n\
        [json.exception.parse_error.101] parse error at line 1, column 11057:\n\
        syntax error while parsing value - invalid string: missing closing quote";

    fn make_recovery_test_config() -> rockbot::config::AppConfig {
        rockbot::config::AppConfig {
            platform: Default::default(),
            rocketchat: RocketChatSection {
                server: ServerConfig {
                    url: "test.example.com".into(),
                    username: "bot".into(),
                    password: "secret".into(),
                    debug: false,
                },
                model: None,
            },
            matrix: None,
            model: ModelConfig {
                default_provider: ProviderName::try_new("mock".to_string()).unwrap(),
                default_model: "mock-model".into(),
                max_iterations: 5,
                max_soul_chars: BoundedUsize::try_new(5000).unwrap(),
                persist_interval_secs: 60,
                memory_ttl_secs: 86400,
                max_context_bytes: BoundedUsize::try_new(4194304).unwrap(),
                max_attachment_bytes: 20971520,
                model_context_length: 1_000_000,
                summarization_enabled: false,
                summarization_ratio: 0.6,
                summarization_target_tokens: 1024,
            },
            chat_providers: vec![ProviderConfig {
                name: ProviderName::try_new("mock".to_string()).unwrap(),
                api_key: "sk-test".into(),
                base_url: ConfigUrl::try_new("https://mock.ai/v1".to_string()).unwrap(),
                basecf_url: None,
                chat_path: Some("/chat/completions".into()),
                draw_path: None,
                models: HashMap::new(),
            }],
            image_providers: vec![],
            image_model: ImageModelConfig {
                default_provider: ProviderName::try_new("mock".to_string()).unwrap(),
                default_text_model: "mock-img".into(),
                default_edit_model: "mock-img-edit".into(),
                default_quality: "standard".into(),
                default_output_format: "png".into(),
                default_num_images: 1,
                default_image_size: "1024x1024".into(),
                default_image_size_tier: "1K".into(),
                default_enable_safety_checker: false,
            },
            tools: HashMap::new(),
            search: Default::default(),
            webdav: None,
            agent: Default::default(),
            acp: None,
        }
    }

    /// Fails with the nlohmann tool-call parse error on the first call, then
    /// succeeds. Records every received request for assertions.
    struct ParseErrorThenSuccessMock {
        requests: Arc<Mutex<Vec<ChatRequest>>>,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl AiProvider for ParseErrorThenSuccessMock {
        async fn complete(
            &self,
            request: ChatRequest,
        ) -> Result<CompletionResult, RockBotError> {
            self.requests.lock().unwrap().push(request.clone());
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err(RockBotError::ServerError {
                    status: 500,
                    body: NLOHMANN_PARSE_ERROR.to_string(),
                })
            } else {
                Ok(CompletionResult {
                    text: Some("Recovered reply".into()),
                    tool_calls: vec![],
                    finish: FinishReason::Stop,
                    reasoning_content: None,
                    usage: None,
                })
            }
        }

        fn provider_name(&self) -> &str {
            "recovery-mock"
        }

        fn model_name(&self) -> &str {
            "recovery-model"
        }
    }

    /// Fails with the nlohmann tool-call parse error on every call.
    struct AlwaysParseErrorMock {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AiProvider for AlwaysParseErrorMock {
        async fn complete(&self, _request: ChatRequest) -> Result<CompletionResult, RockBotError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(RockBotError::ServerError {
                status: 500,
                body: NLOHMANN_PARSE_ERROR.to_string(),
            })
        }

        fn provider_name(&self) -> &str {
            "always-parse-error"
        }

        fn model_name(&self) -> &str {
            "always-parse-error-model"
        }
    }

    /// Happy path (§2k): the provider's tool-call parse error 500 triggers a
    /// history repair + one retry; the retry succeeds and the user receives
    /// the normal reply. Seeded malformed history args are repaired.
    #[tokio::test]
    async fn test_tool_call_parse_error_recovers_and_retries() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let provider = Box::new(ParseErrorThenSuccessMock {
            requests: requests.clone(),
            calls: AtomicUsize::new(0),
        });
        let mut harness = AgentHarness::new(
            make_recovery_test_config(),
            provider,
            None,
            image_cache,
            "@testbot",
        );

        // Seed history with a truncated tool call argument (the 11K-char
        // report scenario) — the recovery path must repair it in place.
        {
            let room = harness
                .memory_mut()
                .get_or_create("room1", "general", "", false);
            room.history.append(ChatMessage::assistant_with_tool_calls(
                "",
                vec![ToolCall::new(
                    "call_001",
                    "save_knowledge",
                    r#"{"content":"FHIR implementation report truncated mid-..."#.to_string(),
                )],
                None,
            ));
        }

        let reply = harness
            .process_message("room1", "general", "General", false, "user", "hi", &[], &[])
            .await
            .unwrap();

        assert_eq!(reply.as_deref(), Some("Recovered reply"));

        let reqs = requests.lock().unwrap();
        assert_eq!(
            reqs.len(),
            2,
            "provider should be called exactly twice (original + recovery retry)"
        );

        // The malformed args in history were repaired by the recovery path.
        let room = harness.memory().get("room1").unwrap();
        let repaired = room
            .history
            .messages
            .iter()
            .filter_map(|m| m.tool_calls.as_ref())
            .flatten()
            .find(|tc| tc.id == "call_001")
            .expect("seeded tool call still present");
        serde_json::from_str::<serde_json::Value>(&repaired.function.arguments)
            .expect("history tool call args must be parseable after repair");
        assert!(repaired.function.arguments.contains("FHIR implementation report"));
    }

    /// Non-parse provider errors are NOT recovered — single call, immediate
    /// fallback reply.
    #[tokio::test]
    async fn test_non_parse_error_not_recovered() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Box::new(NonParseErrorMock {
            calls: calls.clone(),
        });
        let mut harness = AgentHarness::new(
            make_recovery_test_config(),
            provider,
            None,
            image_cache,
            "@testbot",
        );

        let reply = harness
            .process_message("room1", "general", "General", false, "user", "hi", &[], &[])
            .await
            .unwrap();
        let text = reply.unwrap();
        assert!(text.contains("I encountered an error"), "got: {text}");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "no retry for generic errors");
    }

    struct NonParseErrorMock {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AiProvider for NonParseErrorMock {
        async fn complete(&self, _request: ChatRequest) -> Result<CompletionResult, RockBotError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(RockBotError::ServerError {
                status: 500,
                body: "Internal server error".to_string(),
            })
        }

        fn provider_name(&self) -> &str {
            "non-parse-error"
        }

        fn model_name(&self) -> &str {
            "non-parse-error-model"
        }
    }

    /// Retry limit (§2k): if the provider keeps failing with the same parse
    /// error, the harness retries exactly once, then sends the fallback error
    /// reply (no infinite loop).
    #[tokio::test]
    async fn test_tool_call_parse_error_retries_once_then_fallback() {
        let image_cache = Arc::new(rockbot::image_cache::ImageCache::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Box::new(AlwaysParseErrorMock {
            calls: calls.clone(),
        });
        let mut harness = AgentHarness::new(
            make_recovery_test_config(),
            provider,
            None,
            image_cache,
            "@testbot",
        );

        let reply = harness
            .process_message("room1", "general", "General", false, "user", "hi", &[], &[])
            .await
            .unwrap();
        let text = reply.unwrap();
        assert!(
            text.contains("I encountered an error"),
            "fallback reply expected, got: {text}"
        );
        // 1 original call + 1 recovery retry — no more (no infinite loop).
        assert_eq!(calls.load(Ordering::SeqCst), 2, "exactly one recovery retry");
    }
}

// ─── Mock HTTP Tests: ImageGenTool per-call model selection (issue #92) ──────

async fn mount_fal_queue_pipeline(mock_server: &MockServer, base: &str, model_id: &str) {
    Mock::given(method("POST"))
        .and(path(format!("/{model_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "request_id": "req-model-1",
            "status_url": format!("{base}/{model_id}/requests/req-model-1/status"),
            "response_url": format!("{base}/{model_id}/requests/req-model-1"),
        })))
        .expect(1)
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("/{model_id}/requests/req-model-1/status")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "COMPLETED"
        })))
        .expect(1)
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("/{model_id}/requests/req-model-1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "images": [{"url": format!("{base}/result.png"), "width": 1024, "height": 1024}]
        })))
        .expect(1)
        .mount(mock_server)
        .await;
}

async fn mount_result_image_and_webdav(mock_server: &MockServer, base: &str) {
    Mock::given(method("GET"))
        .and(path("/result.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "image/png")
                .set_body_bytes(vec![0x89, 0x50, 0x4E, 0x47]),
        )
        .mount(mock_server)
        .await;

    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(201))
        .mount(mock_server)
        .await;
}

#[tokio::test]
async fn test_image_gen_tool_model_override_uses_catalog_model() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();
    let overridden_model = "bytedance/seedream/v5/pro/text-to-image";
    let default_model = "fal-ai/flux/schnell";

    // Only the OVERRIDDEN model path is mounted — if the tool resolved the
    // alias to anything else, the submit POST would 404 and the test fails.
    mount_fal_queue_pipeline(&mock_server, &base, overridden_model).await;
    mount_result_image_and_webdav(&mock_server, &base).await;

    let mut img_cfg = make_fal_config(&base);
    img_cfg.models = HashMap::from([
        ("seedream5".to_string(), overridden_model.to_string()),
        ("flux".to_string(), default_model.to_string()),
    ]);
    let provider = FalAiProvider::new(&img_cfg, default_model).unwrap();
    let catalog = ImageModelCatalog::new(img_cfg.models.clone(), "flux", "flux");
    let tool = ImageGenTool::new(
        Box::new(provider),
        catalog,
        "medium".into(),
        "png".into(),
        1,
        "4K".into(),
        webdav::WebDavClient::new(&base, "user", "pass").unwrap(),
        Arc::new(rockbot::image_cache::ImageCache::new()),
    );

    let result = tool
        .execute(
            r#"{"prompt":"a cyberpunk city","aspect_ratio":"16:9","model":"seedream5","room_id":"d-abc"}"#,
        )
        .await
        .unwrap();

    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["ok"], true);
    assert!(
        v["image_key"].as_str().is_some(),
        "result must carry an image_key: {result}"
    );
}

#[tokio::test]
async fn test_image_gen_tool_model_omitted_uses_provider_default() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();
    let default_model = "fal-ai/flux/schnell";

    // Only the DEFAULT path is mounted — omitting `model` must hit the
    // provider's configured model with no model_id override.
    mount_fal_queue_pipeline(&mock_server, &base, default_model).await;
    mount_result_image_and_webdav(&mock_server, &base).await;

    let mut img_cfg = make_fal_config(&base);
    img_cfg.models = HashMap::from([
        ("seedream5".to_string(), "bytedance/seedream/v5/pro/text-to-image".to_string()),
        ("flux".to_string(), default_model.to_string()),
    ]);
    let provider = FalAiProvider::new(&img_cfg, default_model).unwrap();
    let catalog = ImageModelCatalog::new(img_cfg.models.clone(), "flux", "flux");
    let tool = ImageGenTool::new(
        Box::new(provider),
        catalog,
        "medium".into(),
        "png".into(),
        1,
        "4K".into(),
        webdav::WebDavClient::new(&base, "user", "pass").unwrap(),
        Arc::new(rockbot::image_cache::ImageCache::new()),
    );

    let result = tool
        .execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","room_id":"d-abc"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["ok"], true);
}

#[tokio::test]
async fn test_image_gen_tool_unknown_model_alias_fails_before_http() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    let mut img_cfg = make_fal_config(&base);
    img_cfg.models = HashMap::from([("flux".to_string(), "fal-ai/flux/schnell".to_string())]);
    let provider = FalAiProvider::new(&img_cfg, "fal-ai/flux/schnell").unwrap();
    let catalog = ImageModelCatalog::new(img_cfg.models.clone(), "flux", "flux");
    let tool = ImageGenTool::new(
        Box::new(provider),
        catalog,
        "medium".into(),
        "png".into(),
        1,
        "4K".into(),
        webdav::WebDavClient::new(&base, "user", "pass").unwrap(),
        Arc::new(rockbot::image_cache::ImageCache::new()),
    );

    let err = tool
        .execute(r#"{"prompt":"a cat","aspect_ratio":"1:1","model":"nope"}"#)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not in image model catalog"));
}

#[tokio::test]
async fn test_openrouter_image_gen_model_id_override_in_body() {
    let mock_server = MockServer::start().await;
    let base = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_string_contains("\"model\":\"qwen/qwen-image-3-pro\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "images": [{
                        "image_url": { "url": "data:image/png;base64,AAAA" }
                    }]
                }
            }]
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = make_openrouter_image_config(&base);
    let provider = OpenRouterImageProvider::new(&config, "microsoft/mai-image-2.5").unwrap();
    let mut params = ImageGenParams::new("a sunset");
    params.model_id = Some("qwen/qwen-image-3-pro".into());
    let bytes = provider.generate_image(&params).await.unwrap();
    assert!(!bytes.is_empty());
}

// ─── Dynamic tool description from [image_providers] config (issue #95) ──────
//
// The tool's description and schema are generated from the catalog at registry
// time — config is the single source of truth. No wiremock served data is
// needed: these tests assert the registry-time derivation (DFD
// _dfd/tools/image-gen/level-2/tool-description.md).

#[test]
fn test_image_gen_description_and_schema_derived_from_config() {
    let models = HashMap::from([
        ("seedream5".to_string(), "bytedance/seedream/v5/pro/text-to-image".to_string()),
        ("mai".to_string(), "microsoft/mai-image-2.5".to_string()),
    ]);
    let catalog = ImageModelCatalog::new(models, "mai", "mai");
    let tool = ImageGenTool::new(
        Box::new(rockbot::provider::fal::FalAiProvider::new(
            &rockbot::config::ProviderConfig {
                name: rockbot::validated::ProviderName::try_new("fal".to_string()).unwrap(),
                api_key: "test-key".into(),
                base_url: rockbot::validated::ConfigUrl::try_new("https://queue.fal.run".to_string()).unwrap(),
                basecf_url: None,
                chat_path: None,
                draw_path: None,
                models: HashMap::new(),
            },
            "bytedance/seedream/v5/pro/text-to-image",
        )
        .unwrap()),
        catalog,
        "medium".into(),
        "png".into(),
        1,
        "4K".into(),
        webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap(),
        Arc::new(rockbot::image_cache::ImageCache::new()),
    );

    let desc = tool.description();
    assert!(desc.contains("Available image models: mai (microsoft/mai-image-2.5), seedream5 (bytedance/seedream/v5/pro/text-to-image)"), "desc: {desc}");
    assert!(desc.contains("Defaults: text-to-image 'mai' / edit 'mai'"), "desc: {desc}");
    assert!(desc.contains("'auto_2K'"), "seedream configured → auto hint in tool description: {desc}");

    let params = tool.parameters();
    assert_eq!(
        params["properties"]["model"]["enum"],
        serde_json::json!(["mai", "seedream5"])
    );
    let aspect_desc = params["properties"]["aspect_ratio"]["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(aspect_desc.contains("auto_2K") && aspect_desc.contains("auto_1K"), "aspect_desc: {aspect_desc}");

    let model_desc = params["properties"]["model"]["description"].as_str().unwrap();
    assert!(model_desc.contains("mai") && model_desc.contains("seedream5"), "model_desc: {model_desc}");
    assert!(model_desc.contains("Default: 'mai' (text-to-image) / 'mai' (edit)"), "model_desc: {model_desc}");
}

#[test]
fn test_image_gen_description_no_auto_hint_without_seedream() {
    let models = HashMap::from([("flux".to_string(), "fal-ai/flux/schnell".to_string())]);
    let catalog = ImageModelCatalog::new(models, "flux", "flux");
    let tool = ImageGenTool::new(
        Box::new(MockImageProviderStub),
        catalog,
        "medium".into(),
        "png".into(),
        1,
        "4K".into(),
        webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap(),
        Arc::new(rockbot::image_cache::ImageCache::new()),
    );

    let desc = tool.description();
    assert!(!desc.contains("auto_2K") && !desc.contains("auto_1K"), "no seedream → no auto hint: {desc}");
    let params = tool.parameters();
    let aspect_desc = params["properties"]["aspect_ratio"]["description"]
        .as_str()
        .unwrap();
    assert!(!aspect_desc.contains("auto_2K"), "aspect_desc: {aspect_desc}");
}

/// Minimal `ImageProvider` holding no state — registry-time schema tests only
/// (never calls `generate_image`).
struct MockImageProviderStub;

#[async_trait::async_trait]
impl rockbot::provider::ImageProvider for MockImageProviderStub {
    async fn generate_image(&self, _params: &ImageGenParams) -> rockbot::Result<Vec<u8>> {
        unreachable!("schema-derived tests never generate")
    }
    async fn upload_file(&self, _data: &[u8], _content_type: &str) -> rockbot::Result<String> {
        unreachable!("schema-derived tests never upload")
    }
    fn provider_name(&self) -> &str {
        "stub"
    }
    fn model_id(&self) -> &str {
        "stub-model"
    }
}

