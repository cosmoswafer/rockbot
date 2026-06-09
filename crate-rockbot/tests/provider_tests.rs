use rockbot::config::{AppConfig, ProviderConfig};
use rockbot::error::{Result, RockBotError};
use rockbot::provider::{AiProvider, DeepSeekProvider, OpenRouterProvider};
use rockbot::types::{
    ChatMessage, ChatRequest, CompletionResult, ContentPart, FinishReason, ImageUrlPayload,
    MessageContent, Role, ThinkingConfig, ToolCall, ToolDef, UsageInfo,
};

use std::collections::HashMap;

// ─── Config Tests ────────────────────────────────────────────────────────────

#[test]
fn test_config_from_example_toml() {
    let toml_content = r#"
[rocketchat]
url = "your-server.example.com"
username = "bot"
password = "secret"
debug = false
default_provider = "openrouter"
default_model = "deepseek"
tools = false
max_history_size = 12
max_text_length = 50000

[[providers]]
name = "openrouter"
api_key = "sk-or-v1-test"
base_url = "https://openrouter.ai/api/v1"
chat_path = "/chat/completions"

[providers.models]
gpt = "openai/gpt-oss-120b:online"
deepseek = "deepseek/deepseek-v3.2:online"

[[providers]]
name = "deepseek"
api_key = "sk-deepseek-test"
base_url = "https://api.deepseek.com/v1"

[providers.models]
chat = "deepseek-chat"
reasoner = "deepseek-reasoner"
"#;
    let config = AppConfig::from_str(toml_content).unwrap();

    assert_eq!(config.rocketchat.default_provider, "openrouter");
    assert_eq!(config.rocketchat.default_model, "deepseek");
    assert_eq!(config.rocketchat.max_history_size, 12);
    assert_eq!(config.rocketchat.max_text_length, 50000);

    assert_eq!(config.providers.len(), 2);

    let openrouter = &config.providers[0];
    assert_eq!(openrouter.name, "openrouter");
    assert_eq!(openrouter.base_url, "https://openrouter.ai/api/v1");
    assert_eq!(openrouter.chat_path.as_deref(), Some("/chat/completions"));
    assert_eq!(
        openrouter.models.get("deepseek").unwrap(),
        "deepseek/deepseek-v3.2:online"
    );

    let deepseek = &config.providers[1];
    assert_eq!(deepseek.name, "deepseek");
    assert_eq!(deepseek.base_url, "https://api.deepseek.com/v1");
    assert_eq!(deepseek.models.get("chat").unwrap(), "deepseek-chat");
    assert_eq!(
        deepseek.models.get("reasoner").unwrap(),
        "deepseek-reasoner"
    );
}

#[test]
fn test_config_find_provider() {
    let toml = r#"
[rocketchat]
url = "test.example.com"
username = "bot"
password = "secret"
debug = false
default_provider = "openrouter"
default_model = "deepseek"
tools = false
max_history_size = 12
max_text_length = 50000

[[providers]]
name = "openrouter"
api_key = "sk-or-v1-test"
base_url = "https://openrouter.ai/api/v1"

[providers.models]
gpt = "openai/gpt-oss-120b:online"

[[providers]]
name = "deepseek"
api_key = "sk-deepseek-test"
base_url = "https://api.deepseek.com/v1"

[providers.models]
chat = "deepseek-chat"
"#;
    let config = AppConfig::from_str(toml).unwrap();

    assert!(config.find_provider("deepseek").is_some());
    assert!(config.find_provider("openrouter").is_some());
    assert!(config.find_provider("nonexistent").is_none());
}

#[test]
fn test_config_resolve_model() {
    let toml = r#"
[rocketchat]
url = "test.example.com"
username = "bot"
password = "secret"
debug = false
default_provider = "openrouter"
default_model = "deepseek"
tools = false
max_history_size = 12
max_text_length = 50000

[[providers]]
name = "openrouter"
api_key = "sk-or-v1-test"
base_url = "https://openrouter.ai/api/v1"

[providers.models]
gpt = "openai/gpt-oss-120b:online"
deepseek = "deepseek/deepseek-v3.2:online"

[[providers]]
name = "deepseek"
api_key = "sk-deepseek-test"
base_url = "https://api.deepseek.com/v1"

[providers.models]
chat = "deepseek-chat"
reasoner = "deepseek-reasoner"
"#;
    let config = AppConfig::from_str(toml).unwrap();

    assert_eq!(
        config.resolve_model("deepseek", "chat").unwrap(),
        "deepseek-chat"
    );
    assert_eq!(
        config.resolve_model("deepseek", "reasoner").unwrap(),
        "deepseek-reasoner"
    );
    assert_eq!(
        config.resolve_model("openrouter", "deepseek").unwrap(),
        "deepseek/deepseek-v3.2:online"
    );
    assert!(config.resolve_model("deepseek", "nonexistent").is_none());
}

#[test]
fn test_provider_chat_url_default() {
    let config = ProviderConfig {
        name: "test".into(),
        api_key: "sk-test".into(),
        base_url: "https://api.example.com".into(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models: HashMap::new(),
    };
    assert_eq!(
        config.chat_url(),
        "https://api.example.com/chat/completions"
    );
}

#[test]
fn test_provider_chat_url_custom() {
    let config = ProviderConfig {
        name: "test".into(),
        api_key: "sk-test".into(),
        base_url: "https://api.example.com/v1".into(),
        basecf_url: None,
        chat_path: Some("/v2/chat".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    assert_eq!(config.chat_url(), "https://api.example.com/v1/v2/chat");
}

#[test]
fn test_provider_chat_url_trailing_slash() {
    let config = ProviderConfig {
        name: "test".into(),
        api_key: "sk-test".into(),
        base_url: "https://api.example.com/".into(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models: HashMap::new(),
    };
    assert_eq!(
        config.chat_url(),
        "https://api.example.com/chat/completions"
    );
}

// ─── Types Tests ─────────────────────────────────────────────────────────────

#[test]
fn test_chat_message_system() {
    let msg = ChatMessage::system("You are a bot");
    assert_eq!(msg.role, Role::System);
    assert_eq!(msg.text_content(), Some("You are a bot"));
}

#[test]
fn test_chat_message_user() {
    let msg = ChatMessage::user("Hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.text_content(), Some("Hello"));
}

#[test]
fn test_chat_message_assistant() {
    let msg = ChatMessage::assistant("Hi there");
    assert_eq!(msg.role, Role::Assistant);
    assert_eq!(msg.text_content(), Some("Hi there"));
}

#[test]
fn test_chat_message_tool() {
    let msg = ChatMessage::tool("call_001", "24°C");
    assert_eq!(msg.role, Role::Tool);
    assert_eq!(msg.tool_call_id, Some("call_001".into()));
}

#[test]
fn test_chat_message_assistant_with_tool_calls() {
    let tc = ToolCall::new("call_1", "get_weather", r#"{"location":"Beijing"}"#);
    let msg = ChatMessage::assistant_with_tool_calls("", vec![tc], Some("Let me think...".into()));
    assert_eq!(msg.role, Role::Assistant);
    assert!(msg.tool_calls.is_some());
    assert_eq!(msg.reasoning_content, Some("Let me think...".into()));
}

#[test]
fn test_tool_call_new() {
    let tc = ToolCall::new("id_1", "search", r#"{"query":"rust"}"#);
    assert_eq!(tc.id, "id_1");
    assert_eq!(tc.function.name, "search");
    assert_eq!(tc.function.arguments, r#"{"query":"rust"}"#);
}

#[test]
fn test_tool_def_new() {
    let td = ToolDef::new(
        "get_weather",
        "Get the weather for a location",
        serde_json::json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            }
        }),
    );
    assert_eq!(td.function.name, "get_weather");
    assert_eq!(
        td.function.description.as_deref(),
        Some("Get the weather for a location")
    );
    assert!(td.function.parameters.is_some());
}

#[test]
fn test_thinking_config() {
    let enabled = ThinkingConfig::enabled();
    assert_eq!(enabled.thinking_type, "enabled");

    let disabled = ThinkingConfig::disabled();
    assert_eq!(disabled.thinking_type, "disabled");
}

#[test]
fn test_chat_request_json_serialization() {
    let request = ChatRequest {
        model: "deepseek-v4-pro".into(),
        messages: vec![ChatMessage::user("Hello")],
        tools: None,
        stream: false,
        temperature: Some(0.7),
        max_tokens: None,
        thinking: Some(ThinkingConfig::enabled()),
        reasoning_effort: Some("high".into()),
        tool_choice: None,
    };

    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["model"], "deepseek-v4-pro");
    let temp = json["temperature"].as_f64().unwrap_or(0.0);
    assert!((temp - 0.7).abs() < 0.001, "temperature was {temp}");
    assert_eq!(json["thinking"]["type"], "enabled");
    assert_eq!(json["reasoning_effort"], "high");
    assert!(json.get("max_tokens").is_none());
}

#[test]
fn test_chat_request_stream_present() {
    let request = ChatRequest {
        model: "test".into(),
        messages: vec![],
        tools: None,
        stream: true,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["stream"].as_bool(), Some(true));
}

#[test]
fn test_chat_request_stream_omitted_when_false() {
    let request = ChatRequest {
        model: "test".into(),
        messages: vec![],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        tool_choice: None,
    };
    let json = serde_json::to_value(&request).unwrap();
    assert!(json.get("stream").is_none());
}

#[test]
fn test_completion_result_defaults() {
    let result = CompletionResult {
        text: Some("Hello".into()),
        tool_calls: vec![],
        finish: FinishReason::Stop,
        reasoning_content: None,
        usage: None,
    };
    assert_eq!(result.text, Some("Hello".into()));
    assert!(result.tool_calls.is_empty());
}

#[test]
fn test_finish_reason_serde() {
    assert_eq!(
        serde_json::to_string(&FinishReason::Stop).unwrap(),
        r#""stop""#
    );
    assert_eq!(
        serde_json::to_string(&FinishReason::ToolUse).unwrap(),
        r#""tool_calls""#
    );
    assert_eq!(
        serde_json::to_string(&FinishReason::Length).unwrap(),
        r#""length""#
    );
}

#[test]
fn test_role_serde() {
    assert_eq!(serde_json::to_string(&Role::System).unwrap(), r#""system""#);
    assert_eq!(
        serde_json::to_string(&Role::Assistant).unwrap(),
        r#""assistant""#
    );
    assert_eq!(serde_json::to_string(&Role::User).unwrap(), r#""user""#);
    assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), r#""tool""#);
}

#[test]
fn test_message_content_text() {
    let content = MessageContent::Text("hello".into());
    let json = serde_json::to_value(&content).unwrap();
    assert_eq!(json.as_str().unwrap(), "hello");
}

#[test]
fn test_message_content_multipart() {
    let content = MessageContent::Multipart(vec![
        ContentPart::Text {
            text: "What's in this image?".into(),
        },
        ContentPart::ImageUrl {
            image_url: ImageUrlPayload {
                url: "https://example.com/img.png".into(),
                detail: Some("high".into()),
            },
        },
    ]);
    let json = serde_json::to_value(&content).unwrap();
    assert_eq!(json[0]["type"], "text");
    assert_eq!(json[0]["text"], "What's in this image?");
    assert_eq!(json[1]["type"], "image_url");
}

#[test]
fn test_content_part_image_url_serde() {
    let part = ContentPart::ImageUrl {
        image_url: ImageUrlPayload {
            url: "data:image/png;base64,abc123".into(),
            detail: None,
        },
    };
    let json = serde_json::to_value(&part).unwrap();
    assert_eq!(json["type"], "image_url");
    assert_eq!(json["image_url"]["url"], "data:image/png;base64,abc123");
}

#[test]
fn test_usage_info() {
    let usage = UsageInfo {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    assert_eq!(usage.total_tokens, 150);
}

#[test]
fn test_tool_call_serde() {
    let json = serde_json::json!({
        "id": "call_00_kw66",
        "type": "function",
        "function": {
            "name": "get_weather",
            "arguments": "{\"location\":\"Hangzhou\"}"
        }
    });
    let tc: ToolCall = serde_json::from_value(json).unwrap();
    assert_eq!(tc.id, "call_00_kw66");
    assert_eq!(tc.function.name, "get_weather");
    assert_eq!(tc.function.arguments, r#"{"location":"Hangzhou"}"#);
}

#[test]
fn test_tool_def_serde() {
    let td = ToolDef::new(
        "search",
        "Search the web",
        serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    );
    let json = serde_json::to_value(&td).unwrap();
    assert_eq!(json["type"], "function");
    assert_eq!(json["function"]["name"], "search");
    assert!(json["function"].get("strict").is_none());
}

#[test]
fn test_tool_def_with_strict() {
    let json = serde_json::json!({
        "type": "function",
        "function": {
            "name": "calc",
            "description": "Calculate something",
            "parameters": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "strict": true
        }
    });
    let td: ToolDef = serde_json::from_value(json).unwrap();
    assert_eq!(td.function.name, "calc");
    assert_eq!(td.function.strict, Some(true));
}

// ─── Error Tests ─────────────────────────────────────────────────────────────

#[test]
fn test_error_display() {
    let err = RockBotError::Provider("test error".into());
    assert_eq!(format!("{}", err), "Provider error: test error");

    let err = RockBotError::AuthFailed("bad key".into());
    assert_eq!(format!("{}", err), "Authentication failed: bad key");

    let err = RockBotError::RateLimited {
        retry_after: Some(30),
    };
    assert!(format!("{}", err).contains("Rate limited"));

    let err = RockBotError::MissingApiKey("openrouter".into());
    assert!(format!("{}", err).contains("openrouter"));
}

#[test]
fn test_error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err: RockBotError = io_err.into();
    assert!(matches!(err, RockBotError::Io(_)));
}

#[test]
fn test_error_result_type() {
    let result: Result<i32> = Ok(42);
    assert_eq!(result.unwrap(), 42);

    let result: Result<i32> = Err(RockBotError::EmptyResponse);
    assert!(result.is_err());
}

// ─── Provider Construction Tests ─────────────────────────────────────────────

#[test]
fn test_deepseek_provider_new_success() {
    let mut models = HashMap::new();
    models.insert("chat".into(), "deepseek-chat".into());

    let config = ProviderConfig {
        name: "deepseek".into(),
        api_key: "sk-real-key-123".into(),
        base_url: "https://api.deepseek.com/v1".into(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models,
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-chat").unwrap();
    assert_eq!(provider.provider_name(), "deepseek");
    assert_eq!(provider.model_name(), "deepseek-chat");
}

#[test]
fn test_deepseek_provider_with_client() {
    let config = ProviderConfig {
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: "https://api.deepseek.com/v1".into(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models: HashMap::new(),
    };
    let client = reqwest::Client::new();
    let provider = DeepSeekProvider::with_client(&config, "deepseek-v4-pro", client).unwrap();
    assert_eq!(provider.model_name(), "deepseek-v4-pro");
}

#[test]
fn test_openrouter_provider_new_success() {
    let config = ProviderConfig {
        name: "openrouter".into(),
        api_key: "sk-or-v1-real".into(),
        base_url: "https://openrouter.ai/api/v1".into(),
        basecf_url: None,
        chat_path: Some("/chat/completions".into()),
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = OpenRouterProvider::new(&config, "openai/gpt-4").unwrap();
    assert_eq!(provider.provider_name(), "openrouter");
    assert_eq!(provider.model_name(), "openai/gpt-4");
}

#[test]
fn test_deepseek_new_rejects_editme_key() {
    let config = ProviderConfig {
        name: "deepseek".into(),
        api_key: "EDITME".into(),
        base_url: "https://api.deepseek.com/v1".into(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models: HashMap::new(),
    };
    let result = DeepSeekProvider::new(&config, "chat");
    assert!(result.is_err());
    match result {
        Err(RockBotError::MissingApiKey(_)) => {}
        _ => panic!("Expected MissingApiKey"),
    }
}

#[test]
fn test_openrouter_new_rejects_empty_key() {
    let config = ProviderConfig {
        name: "openrouter".into(),
        api_key: "".into(),
        base_url: "https://openrouter.ai/api/v1".into(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models: HashMap::new(),
    };
    let result = OpenRouterProvider::new(&config, "gpt");
    assert!(result.is_err());
    match result {
        Err(RockBotError::MissingApiKey(_)) => {}
        _ => panic!("Expected MissingApiKey"),
    }
}

// ─── Trait Object Tests ──────────────────────────────────────────────────────

#[test]
fn test_ai_provider_is_object_safe() {
    let config = ProviderConfig {
        name: "deepseek".into(),
        api_key: "sk-key".into(),
        base_url: "https://api.deepseek.com/v1".into(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = DeepSeekProvider::new(&config, "deepseek-v4-pro").unwrap();
    // Verify it can be used as a trait object
    let _: &dyn AiProvider = &provider;
    assert_eq!(provider.provider_name(), "deepseek");
    assert_eq!(provider.model_name(), "deepseek-v4-pro");
}

// ─── ChatMessage Multipart Tests ─────────────────────────────────────────────

#[test]
fn test_chat_message_user_multipart() {
    let msg = ChatMessage {
        role: Role::User,
        content: MessageContent::Multipart(vec![
            ContentPart::Text {
                text: "Describe this image".into(),
            },
            ContentPart::ImageUrl {
                image_url: ImageUrlPayload {
                    url: "https://example.com/photo.jpg".into(),
                    detail: Some("auto".into()),
                },
            },
        ]),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    };
    assert!(msg.text_content().is_none());
    match &msg.content {
        MessageContent::Multipart(parts) => {
            assert_eq!(parts.len(), 2);
        }
        _ => panic!("Expected multipart"),
    }
}
