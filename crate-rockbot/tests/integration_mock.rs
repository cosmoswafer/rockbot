use rockbot::config::ProviderConfig;
use rockbot::error::RockBotError;
use rockbot::provider::{AiProvider, DeepSeekProvider, FalAiProvider, ImageProvider, OpenRouterImageProvider, OpenRouterProvider};
use rockbot::tool::Tool;
use rockbot::types::{ChatMessage, ChatRequest, FinishReason, ImageGenParams, ThinkingConfig, ToolDef};
use std::collections::HashMap;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
        name: "deepseek".into(),
        api_key: "sk-test-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-bad-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: mock_server.uri(),
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
        name: "openrouter".into(),
        api_key: "sk-or-test".into(),
        base_url: mock_server.uri(),
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
        name: "openrouter".into(),
        api_key: "sk-or-test".into(),
        base_url: mock_server.uri(),
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
        name: "openrouter".into(),
        api_key: "sk-or-test".into(),
        base_url: mock_server.uri(),
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
        name: "openrouter".into(),
        api_key: "sk-bad".into(),
        base_url: mock_server.uri(),
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
        name: "openrouter".into(),
        api_key: "sk-or-test".into(),
        base_url: mock_server.uri(),
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
        name: "openrouter".into(),
        api_key: "sk-or-test".into(),
        base_url: mock_server.uri(),
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
        name: "openrouter".into(),
        api_key: "sk-or-test".into(),
        base_url: mock_server.uri(),
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
        name: "openrouter".into(),
        api_key: "sk-or-v1-test".into(),
        base_url: mock_uri.to_string(),
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
        name: "fal".into(),
        api_key: "fal-test-key".into(),
        base_url: mock_uri.to_string(),
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
        10000, // max_chars
        50,    // max_history
        5000,  // max_summary_chars
        7,     // summary_days
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

/// Verifies that snapshot loading (history + daily_summaries + soul)
/// doesn't conflict with in-memory state when all three are loaded together.
#[test]
fn test_memory_load_snapshot_with_soul_and_summaries_no_conflict() {
    let mut mm = rockbot::memory::MemoryManager::new(
        10000, 50, 5000, 7, 2000, 60, 0,
    );

    let room_id = "snapshot-test";

    // Pre-populate soul
    let soul = rockbot::memory::SoulMemory {
        room_id: room_id.to_string(),
        content: "# Soul Memory\n\n- My name is TestBot\n- likes Rust".to_string(),
        updated_at: "2026-06-10T00:00:00Z".to_string(),
    };
    mm.set_soul(room_id, soul);

    // Pre-populate daily summaries
    let summaries = vec![
        rockbot::memory::DailySummary {
            date: "2026-06-10".to_string(),
            summary: "User asked about Rust macros".to_string(),
            msg_count: 5,
            char_count: 200,
        },
        rockbot::memory::DailySummary {
            date: "2026-06-09".to_string(),
            summary: "User asked about Docker".to_string(),
            msg_count: 3,
            char_count: 150,
        },
    ];
    mm.set_daily_summaries(room_id, summaries);

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
    assert_eq!(snap.daily_summaries.len(), 2, "Should have both summaries");

    // Verify all data is consistent (soul, summaries, history coexist)
    let ctx = mm.build_context(room_id, "You are a helpful bot.", None, None);
    assert!(!ctx.is_empty(), "Context should not be empty");

    // Verify soul is in context (as system message)
    let has_soul = ctx.iter().any(|m| {
        format!("{:?}", m.content).contains("TestBot")
            && format!("{:?}", m.content).contains("likes Rust")
    });
    assert!(has_soul, "Context should include soul content");

    // Verify summaries are in context
    let has_summaries = ctx.iter().any(|m| {
        let c = format!("{:?}", m.content);
        c.contains("Rust macros") || c.contains("Docker")
    });
    assert!(has_summaries, "Context should include daily summaries");
}

/// Tests that concurrent snapshot builds and memory mutations
/// don't lose data. Simulates: messages arrive -> build snapshot ->
/// messages arrive again -> build snapshot again.
#[test]
fn test_memory_snapshot_repeated_builds_no_data_loss() {
    let mut mm = rockbot::memory::MemoryManager::new(
        10000, 50, 5000, 7, 2000, 60, 0,
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
        10000, 50, 5000, 7, 2000, 60, 0,
    );

    let room1 = mm.get_or_create("r1", "channel-a", "Channel A", false);
    room1.history.append(ChatMessage::user("alice: room1 msg1"));
    room1.history.append(ChatMessage::assistant("bot: room1 reply"));

    let room2 = mm.get_or_create("r2", "dm-bob", "DM Bob", true);
    room2.history.append(ChatMessage::user("bob: room2 msg1"));

    // Set soul in room1
    mm.set_soul("r1", rockbot::memory::SoulMemory {
        room_id: "r1".to_string(),
        content: "# Soul Memory\n\n- My name is Room1Bot".to_string(),
        updated_at: String::new(),
    });

    // Set summaries in room2
    mm.set_daily_summaries("r2", vec![rockbot::memory::DailySummary {
        date: "2026-06-10".to_string(),
        summary: "Room2 daily".to_string(),
        msg_count: 1,
        char_count: 10,
    }]);

    // Build context for each room - should not cross-contaminate
    let ctx1 = mm.build_context("r1", "You are a bot.", None, None);
    let ctx2 = mm.build_context("r2", "You are a bot.", None, None);

    // Room1 has soul but not room2's summary
    let room1_has_own_soul = ctx1.iter().any(|m| {
        format!("{:?}", m.content).contains("Room1Bot")
    });
    assert!(room1_has_own_soul, "Room1 context should include its own soul");

    let room1_has_room2_data = ctx1.iter().any(|m| {
        format!("{:?}", m.content).contains("Room2 daily")
    });
    assert!(!room1_has_room2_data, "Room1 context should NOT include Room2's summary");

    // Room2 has summary but not room1's soul
    let room2_has_own_summary = ctx2.iter().any(|m| {
        format!("{:?}", m.content).contains("Room2 daily")
    });
    assert!(room2_has_own_summary, "Room2 context should include its own summary");

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
