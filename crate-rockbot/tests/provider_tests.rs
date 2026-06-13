use rockbot::config::{AppConfig, ProviderConfig};
use rockbot::error::{Result, RockBotError};
use rockbot::provider::{AiProvider, DeepSeekProvider, FalAiProvider, OpenRouterProvider};
use validator::Validate;
use rockbot::types::{
    ChatMessage, ChatRequest, CompletionResult, ContentPart, FinishReason, ImageUrlPayload,
    MessageContent, Role, ThinkingConfig, ToolCall, ToolDef, UsageInfo,
};

use std::collections::HashMap;

use rockbot::validated::{ConfigUrl, ProviderName};

// ─── Config Tests ────────────────────────────────────────────────────────────

#[test]
fn test_config_from_example_toml() {
    let toml_content = r#"
[rocketchat.server]
url = "your-server.example.com"
username = "bot"
password = "secret"


[rocketchat.model]
default_provider = "openrouter"
default_model = "deepseek"
max_history_size = 12
max_text_length = 50000

[[chat_providers]]
name = "openrouter"
api_key = "sk-or-v1-test"
base_url = "https://openrouter.ai/api/v1"
chat_path = "/chat/completions"

[chat_providers.models]
gpt = "openai/gpt-oss-120b:online"
deepseek = "deepseek/deepseek-v3.2:online"

[[chat_providers]]
name = "deepseek"
api_key = "sk-deepseek-test"
base_url = "https://api.deepseek.com/v1"

[chat_providers.models]
chat = "deepseek-chat"
reasoner = "deepseek-reasoner"
"#;
    let config = AppConfig::from_toml(toml_content).unwrap();

    assert_eq!(config.rocketchat.model.default_provider.as_str(), "openrouter");
    assert_eq!(config.rocketchat.model.default_model, "deepseek");
    assert_eq!(*config.rocketchat.model.max_history_size, 12);
    assert_eq!(*config.rocketchat.model.max_text_length, 50000);
    assert_eq!(config.rocketchat.model.max_iterations, 28); // default

    assert_eq!(config.chat_providers.len(), 2);

    let openrouter = &config.chat_providers[0];
    assert_eq!(openrouter.name.as_str(), "openrouter");
    assert_eq!(openrouter.base_url.as_str(), "https://openrouter.ai/api/v1");
    assert_eq!(openrouter.chat_path.as_deref(), Some("/chat/completions"));
    assert_eq!(
        openrouter.models.get("deepseek").unwrap(),
        "deepseek/deepseek-v3.2:online"
    );

    let deepseek = &config.chat_providers[1];
    assert_eq!(deepseek.name.as_str(), "deepseek");
    assert_eq!(deepseek.base_url.as_str(), "https://api.deepseek.com/v1");
    assert_eq!(deepseek.models.get("chat").unwrap(), "deepseek-chat");
    assert_eq!(
        deepseek.models.get("reasoner").unwrap(),
        "deepseek-reasoner"
    );
}

#[test]
fn test_config_max_iterations_default() {
    let toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"

[rocketchat.model]
default_provider = "mock"
default_model = "chat"

[[chat_providers]]
name = "mock"
api_key = "sk-mock"
base_url = "https://mock.ai/v1"

[chat_providers.models]
chat = "mock-model"
"#;
    let config = AppConfig::from_toml(toml).unwrap();
    assert_eq!(config.rocketchat.model.max_iterations, 28);
}

#[test]
fn test_config_max_iterations_custom() {
    let toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"

[rocketchat.model]
default_provider = "mock"
default_model = "chat"
max_iterations = 16

[[chat_providers]]
name = "mock"
api_key = "sk-mock"
base_url = "https://mock.ai/v1"

[chat_providers.models]
chat = "mock-model"
"#;
    let config = AppConfig::from_toml(toml).unwrap();
    assert_eq!(config.rocketchat.model.max_iterations, 16);
}

#[test]
fn test_config_find_provider() {
    let toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"


[rocketchat.model]
default_provider = "openrouter"
default_model = "deepseek"
max_history_size = 12
max_text_length = 50000

[[chat_providers]]
name = "openrouter"
api_key = "sk-or-v1-test"
base_url = "https://openrouter.ai/api/v1"

[chat_providers.models]
gpt = "openai/gpt-oss-120b:online"

[[chat_providers]]
name = "deepseek"
api_key = "sk-deepseek-test"
base_url = "https://api.deepseek.com/v1"

[chat_providers.models]
chat = "deepseek-chat"
"#;
    let config = AppConfig::from_toml(toml).unwrap();

    assert!(config.find_chat_provider("deepseek").is_some());
    assert!(config.find_chat_provider("openrouter").is_some());
    assert!(config.find_chat_provider("nonexistent").is_none());
}

#[test]
fn test_config_resolve_model() {
    let toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"


[rocketchat.model]
default_provider = "openrouter"
default_model = "deepseek"
max_history_size = 12
max_text_length = 50000

[[chat_providers]]
name = "openrouter"
api_key = "sk-or-v1-test"
base_url = "https://openrouter.ai/api/v1"

[chat_providers.models]
gpt = "openai/gpt-oss-120b:online"
deepseek = "deepseek/deepseek-v3.2:online"

[[chat_providers]]
name = "deepseek"
api_key = "sk-deepseek-test"
base_url = "https://api.deepseek.com/v1"

[chat_providers.models]
chat = "deepseek-chat"
reasoner = "deepseek-reasoner"
"#;
    let config = AppConfig::from_toml(toml).unwrap();

    assert_eq!(
        config.resolve_chat_model("deepseek", "chat").unwrap(),
        "deepseek-chat"
    );
    assert_eq!(
        config.resolve_chat_model("deepseek", "reasoner").unwrap(),
        "deepseek-reasoner"
    );
    assert_eq!(
        config.resolve_chat_model("openrouter", "deepseek").unwrap(),
        "deepseek/deepseek-v3.2:online"
    );
    assert!(config.resolve_chat_model("deepseek", "nonexistent").is_none());
}

#[test]
fn test_tool_config_deserialize() {
    let toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"


[rocketchat.model]
default_provider = "openrouter"
default_model = "deepseek"
max_history_size = 12
max_text_length = 50000

[[chat_providers]]
name = "openrouter"
api_key = "sk-or-v1-test"
base_url = "https://openrouter.ai/api/v1"

[chat_providers.models]
deepseek = "deepseek/deepseek-v3.2:online"

[tools.exa]
api_key = "exa-key-123"
"#;
    let config = AppConfig::from_toml(toml).unwrap();
    assert_eq!(config.tools.len(), 1);
    let exa = config.tools.get("exa").unwrap();
    assert_eq!(exa.api_key, "exa-key-123");
}

#[test]
fn test_find_tool() {
    let toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"


[rocketchat.model]
default_provider = "openrouter"
default_model = "deepseek"
max_history_size = 12
max_text_length = 50000

[[chat_providers]]
name = "openrouter"
api_key = "sk-or-v1-test"
base_url = "https://openrouter.ai/api/v1"

[chat_providers.models]
deepseek = "deepseek/deepseek-v3.2:online"

[tools.exa]
api_key = "exa-key-123"

[tools.vision]
api_key = "vis-key-456"
"#;
    let config = AppConfig::from_toml(toml).unwrap();

    let exa = config.tools.get("exa");
    assert!(exa.is_some());
    assert_eq!(exa.unwrap().api_key, "exa-key-123");

    let vision = config.tools.get("vision");
    assert!(vision.is_some());
    assert_eq!(vision.unwrap().api_key, "vis-key-456");

    assert!(!config.tools.contains_key("nonexistent"));
}

#[test]
fn test_config_from_file_missing_default() {
    let dir = std::env::temp_dir().join("rockbot_test_missing_default");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let old_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let result = AppConfig::from_file("/nonexistent/default.config.toml");

    std::env::set_current_dir(&old_cwd).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("corrupt") || msg.contains("default"),
        "msg: {msg}"
    );
}

#[test]
fn test_config_merge_user_wins() {
    let dir = std::env::temp_dir().join("rockbot_test_merge_user_wins");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let default_toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"

[rocketchat.model]
default_provider = "p1"
default_model = "chat"
max_iterations = 8

[[chat_providers]]
name = "p1"
api_key = "sk-test"
base_url = "https://test.ai/v1"

[chat_providers.models]
chat = "test-model"
"#;
    std::fs::write(dir.join("default.config.toml"), default_toml).unwrap();

    let user_toml = r#"
[rocketchat.model]
max_iterations = 100
"#;
    let user_path = dir.join("user.config.toml");
    std::fs::write(&user_path, user_toml).unwrap();

    let old_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let config = AppConfig::from_file(
        user_path.to_str().unwrap(),
    )
    .unwrap();

    std::env::set_current_dir(&old_cwd).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(config.rocketchat.model.max_iterations, 100);
    assert_eq!(config.rocketchat.model.default_provider.as_str(), "p1");
}

#[test]
fn test_config_validate_missing_provider() {
    let toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"

[rocketchat.model]
default_provider = "nonexistent"
default_model = "chat"

[[chat_providers]]
name = "p1"
api_key = "sk-test"
base_url = "https://test.ai/v1"
"#;
    let config = AppConfig::from_toml(toml).unwrap();
    let result = config.validate();
    assert!(result.is_err());
}

#[test]
fn test_config_merge_named_arrays_user_overrides() {
    let default_toml = r#"
[[chat_providers]]
name = "p1"
api_key = "default-key"
base_url = "https://default.ai/v1"
"#;
    let user_toml = r#"
[[chat_providers]]
name = "p1"
api_key = "user-key"
base_url = "https://user.ai/v1"
"#;
    let default_value: toml::Value = toml::from_str(default_toml).unwrap();
    let user_value: toml::Value = toml::from_str(user_toml).unwrap();
    let merged = rockbot::merge_toml(default_value, user_value);
    let providers = merged
        .get("chat_providers")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(
        providers[0].get("api_key").unwrap().as_str().unwrap(),
        "user-key"
    );
}

#[test]
fn test_provider_chat_url_default() {
    let config = ProviderConfig {
        name: ProviderName::try_new("test".to_string()).unwrap(),
        api_key: "sk-test".into(),
        base_url: ConfigUrl::try_new("https://api.example.com".to_string()).unwrap(),
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
        name: ProviderName::try_new("test".to_string()).unwrap(),
        api_key: "sk-test".into(),
        base_url: ConfigUrl::try_new("https://api.example.com/v1".to_string()).unwrap(),
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
        name: ProviderName::try_new("test".to_string()).unwrap(),
        api_key: "sk-test".into(),
        base_url: ConfigUrl::try_new("https://api.example.com/".to_string()).unwrap(),
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
    assert!(result.is_ok());
    assert_eq!(result.ok(), Some(42));

    let result: Result<i32> = Err(RockBotError::EmptyResponse);
    assert!(result.is_err());
}

// ─── Provider Construction Tests ─────────────────────────────────────────────

#[test]
fn test_deepseek_provider_new_success() {
    let mut models = HashMap::new();
    models.insert("chat".into(), "deepseek-chat".into());

    let config = ProviderConfig {
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-real-key-123".into(),
        base_url: ConfigUrl::try_new("https://api.deepseek.com/v1".to_string()).unwrap(),
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
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new("https://api.deepseek.com/v1".to_string()).unwrap(),
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
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "sk-or-v1-real".into(),
        base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
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
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "EDITME".into(),
        base_url: ConfigUrl::try_new("https://api.deepseek.com/v1".to_string()).unwrap(),
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
        name: ProviderName::try_new("openrouter".to_string()).unwrap(),
        api_key: "".into(),
        base_url: ConfigUrl::try_new("https://openrouter.ai/api/v1".to_string()).unwrap(),
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
        name: ProviderName::try_new("deepseek".to_string()).unwrap(),
        api_key: "sk-key".into(),
        base_url: ConfigUrl::try_new("https://api.deepseek.com/v1".to_string()).unwrap(),
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

#[test]
fn test_image_provider_is_object_safe() {
    let config = ProviderConfig {
        name: ProviderName::try_new("fal".to_string()).unwrap(),
        api_key: "fal-key".into(),
        base_url: ConfigUrl::try_new("https://queue.fal.run".to_string()).unwrap(),
        basecf_url: None,
        chat_path: None,
        draw_path: None,
        models: HashMap::new(),
    };
    let provider = FalAiProvider::new(&config, "fal-ai/flux/schnell").unwrap();
    // Verify it can be used as a trait object
    let _: &dyn rockbot::provider::ImageProvider = &provider;
    assert_eq!(provider.provider_name(), "fal");
    assert_eq!(provider.model_id(), "fal-ai/flux/schnell");
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
