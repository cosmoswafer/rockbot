use rockbot::config::ProviderConfig;
use rockbot::error::RockBotError;
use rockbot::provider::{AiProvider, DeepSeekProvider, OpenRouterProvider};
use rockbot::types::{ChatMessage, ChatRequest, FinishReason, ThinkingConfig, ToolDef};
use std::collections::HashMap;
use wiremock::matchers::{header, method, path};
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
