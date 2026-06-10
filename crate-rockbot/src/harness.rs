use std::sync::Arc;

use tracing::{debug, error, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::AppConfig;
use crate::error::Result;
use crate::memory::{ArchiveEntry, MemoryManager};
use crate::provider::AiProvider;
use crate::tool::ToolRegistry;
use crate::types::{ChatMessage, ChatRequest, Role};

const DEFAULT_SYSTEM_PROMPT: &str = "\
You are RockBot, a helpful AI assistant running on a RocketChat server. \
You respond to DMs and @mentions concisely and helpfully. \
When you need information from the web, use the web_search tool. \
When you need to fetch a URL, use web_fetch. \
When you need to analyze an image, use the vision tool. \
When you need to read, write, list, or manage files on remote storage, use the webdav tool. \
When you need to generate an image, use the image_gen tool. \
Answer in the same language as the user. \
Keep responses clear and to the point.\
";

pub struct AgentHarness {
    config: Arc<AppConfig>,
    provider: Box<dyn AiProvider>,
    memory: MemoryManager,
    tools: ToolRegistry,
    webdav: Option<WebDavClient>,
    max_iterations: u32,
}

impl AgentHarness {
    pub fn new(
        config: AppConfig,
        provider: Box<dyn AiProvider>,
        webdav: Option<WebDavClient>,
    ) -> Self {
        let max_chars = config.rocketchat.model.max_text_length;
        let max_history = config.rocketchat.model.max_history_size;
        let max_iterations = config.rocketchat.model.max_iterations;
        let config = Arc::new(config);
        Self {
            config,
            provider,
            memory: MemoryManager::new(max_chars, max_history),
            tools: ToolRegistry::new(),
            webdav,
            max_iterations,
        }
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    pub fn register_tool(&mut self, tool: Box<dyn crate::tool::Tool>) {
        self.tools.register(tool);
    }

    pub fn provider(&self) -> &dyn AiProvider {
        self.provider.as_ref()
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn memory(&self) -> &MemoryManager {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut MemoryManager {
        &mut self.memory
    }

    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    pub async fn process_message(
        &mut self,
        room_id: &str,
        room_name: &str,
        is_dm: bool,
        sender_name: &str,
        text: &str,
    ) -> Result<Option<String>> {
        let clean_text = if !is_dm && !text.is_empty() {
            text.trim_start().to_string()
        } else {
            text.to_string()
        };

        let room = self.memory.get_or_create(room_id, room_name, is_dm);
        let user_msg = ChatMessage::user(format!("{}: {}", sender_name, clean_text));
        room.history.append(user_msg);

        let system_prompt = self.build_system_prompt();
        let tool_defs = self.tools.definitions();
        let have_tools = !tool_defs.is_empty();

        let model = self.resolve_model();

        let mut messages = self
            .memory
            .build_context(room_id, &system_prompt, None, None);

        let mut iterations: u32 = 0;

        loop {
            iterations += 1;
            if iterations > self.max_iterations {
                warn!(
                    "Max agent iterations ({}) reached for room {}",
                    self.max_iterations, room_id
                );
                let fallback = "I'm sorry, I got stuck in a loop. Could you rephrase your request?";
                let assistant_msg = ChatMessage::assistant(fallback);
                self.append_to_history(room_id, assistant_msg);
                return Ok(Some(fallback.to_string()));
            }

            let request = ChatRequest {
                model: model.clone(),
                messages,
                tools: if have_tools {
                    Some(tool_defs.clone())
                } else {
                    None
                },
                stream: false,
                temperature: Some(0.7),
                max_tokens: Some(4096),
                thinking: None,
                reasoning_effort: None,
                tool_choice: None,
            };

            match self.provider.complete(request).await {
                Ok(result) => {
                    if !result.tool_calls.is_empty() {
                        let assistant_msg = ChatMessage::assistant_with_tool_calls(
                            "",
                            result.tool_calls.clone(),
                            result.reasoning_content.clone(),
                        );
                        self.append_to_history(room_id, assistant_msg);

                        for tool_call in &result.tool_calls {
                            debug!(
                                "Executing tool {} (call_id: {})",
                                tool_call.function.name, tool_call.id
                            );

                            let arguments = if tool_call.function.name == "webdav"
                                || tool_call.function.name == "image_gen"
                            {
                                let wd = compute_webdav_dir(room_name, is_dm);
                                inject_room_context(&tool_call.function.arguments, room_id, &wd)
                            } else {
                                tool_call.function.arguments.clone()
                            };

                            let tool_result = self
                                .tools
                                .execute_by_name(&tool_call.function.name, &arguments)
                                .await?;

                            let tool_msg = ChatMessage::tool(&tool_call.id, &tool_result.content);
                            self.append_to_history(room_id, tool_msg);
                        }

                        messages = self
                            .memory
                            .build_context(room_id, &system_prompt, None, None);
                        continue;
                    }

                    if let Some(text) = result.text {
                        let clean = text.trim().to_string();
                        let assistant_msg = if clean.is_empty() {
                            ChatMessage::assistant("(no response)")
                        } else {
                            ChatMessage::assistant(&clean)
                        };
                        self.append_to_history(room_id, assistant_msg);

                        let reply = if clean.is_empty() {
                            "I processed your request but received an empty response.".to_string()
                        } else {
                            clean
                        };
                        return Ok(Some(reply));
                    }

                    let fallback = "I received a response but it was empty. Please try again.";
                    let assistant_msg = ChatMessage::assistant(fallback);
                    self.append_to_history(room_id, assistant_msg);
                    return Ok(Some(fallback.to_string()));
                }
                Err(e) => {
                    error!("AI provider error: {}", e);
                    let fallback = format!("I encountered an error: {}. Please try again.", e);
                    let assistant_msg = ChatMessage::assistant(&fallback);
                    self.append_to_history(room_id, assistant_msg);
                    return Ok(Some(fallback));
                }
            }
        }
    }

    fn append_to_history(&mut self, room_id: &str, msg: ChatMessage) {
        if let Some(room) = self.memory.get_mut(room_id) {
            room.history.append(msg);
        }
    }

    fn build_system_prompt(&self) -> String {
        DEFAULT_SYSTEM_PROMPT.to_string()
    }

    fn resolve_model(&self) -> String {
        self.config
            .resolve_model(
                &self.config.rocketchat.model.default_provider,
                &self.config.rocketchat.model.default_model,
            )
            .unwrap_or_else(|| {
                warn!(
                    "Model alias '{}' not found for provider '{}', using raw model name",
                    self.config.rocketchat.model.default_model,
                    self.config.rocketchat.model.default_provider
                );
                self.config.rocketchat.model.default_model.clone()
            })
    }

    pub async fn archive_room_if_needed(&mut self, room_id: &str) -> Result<()> {
        let needs_archive = self.memory.check_and_archive(room_id);
        if let Some((rid, msgs, seq)) = needs_archive {
            if let Some(ref webdav_client) = self.webdav {
                let count = msgs.len();
                let summary = self.summarize_for_archive(&msgs).await;
                let content = format_messages_as_markdown(&summary, &msgs);

                let wd = {
                    let room = self.memory.get(&rid);
                    let (rn, dm) = room
                        .map(|r| (r.room_name.as_str(), r.is_dm))
                        .unwrap_or((&rid, false));
                    compute_webdav_dir(rn, dm)
                };
                let path = WebDavPath::new("").archive_path(&wd, seq);

                match webdav_client
                    .write_file_with_fallback(&path, content.as_bytes().to_vec())
                    .await
                {
                    Ok(()) => {
                        debug!(
                            "Archived {} messages for room {} to {} (summary: {:.60}...)",
                            count, rid, path, summary
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to archive messages for room {}: {}. Truncating instead.",
                            rid, e
                        );
                    }
                }
                self.memory.prune_archived(&rid, count);
            } else {
                debug!(
                    "No WebDAV client, truncating instead of archiving for room {}",
                    rid
                );
                let count = msgs.len();
                self.memory.prune_archived(&rid, count);
            }
        }
        Ok(())
    }

    async fn summarize_for_archive(&self, messages: &[ChatMessage]) -> String {
        let mut summary_parts: Vec<String> = Vec::new();
        for msg in messages.iter().take(5) {
            if let Some(text) = msg.text_content() {
                if !text.is_empty() {
                    let truncated = if text.len() > 80 {
                        format!("{}...", &text[..80])
                    } else {
                        text.to_string()
                    };
                    summary_parts.push(truncated);
                }
            }
        }
        if summary_parts.is_empty() {
            format!("{} messages archived", messages.len())
        } else {
            let preview = summary_parts.join(" | ");
            format!("{} messages: {}", messages.len(), preview)
        }
    }

    pub async fn load_archives_for_room(&self, _room_id: &str) -> Result<Vec<ArchiveEntry>> {
        Ok(Vec::new())
    }
}

fn format_messages_as_markdown(summary: &str, messages: &[ChatMessage]) -> String {
    let mut md = format!(
        "# Conversation Archive\n\n**Summary**: {}\n\n---\n\n",
        summary
    );
    for msg in messages {
        let role = match msg.role {
            Role::System => "System",
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => "Tool",
        };
        let text = msg.text_content().unwrap_or("");
        if !text.is_empty() {
            md.push_str(&format!("**{}**: {}\n\n", role, text));
        }
        if let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                md.push_str(&format!(
                    "**{}**: [tool: {}](args: {})\n\n",
                    role, tc.function.name, tc.function.arguments
                ));
            }
        }
        if text.is_empty() && msg.tool_calls.is_none() {
            md.push_str(&format!("**{}**: (empty message)\n\n", role));
        }
    }
    md.trim().to_string()
}

fn inject_room_context(arguments: &str, room_id: &str, webdav_dir: &str) -> String {
    let mut args: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));
    args["room_id"] = serde_json::Value::String(room_id.to_string());
    args["webdav_dir"] = serde_json::Value::String(webdav_dir.to_string());
    serde_json::to_string(&args).unwrap_or_else(|_| arguments.to_string())
}

fn compute_webdav_dir(room_name: &str, is_dm: bool) -> String {
    if is_dm {
        format!("dms/{}", room_name)
    } else {
        format!("rooms/{}", room_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RockBotError;
    use crate::provider::AiProvider;
    use crate::types::{CompletionResult, FinishReason, ToolCall};
    use async_trait::async_trait;

    struct MockProvider {
        responses: std::sync::Mutex<Vec<CompletionResult>>,
        call_count: std::sync::atomic::AtomicUsize,
    }

    impl MockProvider {
        fn new(responses: Vec<CompletionResult>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl AiProvider for MockProvider {
        async fn complete(&self, _request: ChatRequest) -> Result<CompletionResult> {
            let mut responses = self.responses.lock().unwrap();
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if responses.is_empty() {
                Err(RockBotError::Provider("No mock responses".into()))
            } else {
                Ok(responses.remove(0))
            }
        }

        fn provider_name(&self) -> &str {
            "mock"
        }

        fn model_name(&self) -> &str {
            "mock-model"
        }
    }

    fn make_test_config() -> AppConfig {
        AppConfig::from_toml(
            r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"
debug = false

[rocketchat.model]
default_provider = "mock"
default_model = "chat"
max_history_size = 12
max_text_length = 50000
max_iterations = 8

[[providers]]
name = "mock"
api_key = "sk-mock"
base_url = "https://mock.ai/v1"

[providers.models]
chat = "mock-model"
"#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_harness_simple_response() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![CompletionResult {
            text: Some("Hello! How can I help?".into()),
            tool_calls: vec![],
            finish: FinishReason::Stop,
            reasoning_content: None,
            usage: None,
        }]));

        let mut harness = AgentHarness::new(config, provider, None);
        let result = harness
            .process_message("room1", "general", false, "user", "Hi")
            .await;

        assert!(result.is_ok());
        let reply = result.unwrap();
        assert!(reply.is_some());
        assert!(reply.unwrap().contains("Hello"));
    }

    #[tokio::test]
    async fn test_harness_dm_message() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![CompletionResult {
            text: Some("DM response".into()),
            tool_calls: vec![],
            finish: FinishReason::Stop,
            reasoning_content: None,
            usage: None,
        }]));

        let mut harness = AgentHarness::new(config, provider, None);
        let result = harness
            .process_message("dm-alice", "", true, "alice", "Hello bot")
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), "DM response");
    }

    #[tokio::test]
    async fn test_harness_provider_error() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));

        let mut harness = AgentHarness::new(config, provider, None);
        let result = harness
            .process_message("room1", "general", false, "user", "Hi")
            .await;

        assert!(result.is_ok());
        let reply = result.unwrap();
        assert!(reply.is_some());
        assert!(reply.unwrap().contains("error"));
    }

    #[tokio::test]
    async fn test_harness_max_iterations_limit() {
        let config_toml = r#"
[rocketchat.server]
url = "test.example.com"
username = "bot"
password = "secret"

[rocketchat.model]
default_provider = "mock"
default_model = "chat"
max_iterations = 2

[[providers]]
name = "mock"
api_key = "sk-mock"
base_url = "https://mock.ai/v1"

[providers.models]
chat = "mock-model"
"#;
        let config = AppConfig::from_toml(config_toml).unwrap();

        let tool_call = ToolCall::new("call_1", "web_search", r#"{"query":"test"}"#);
        let tool_result = CompletionResult {
            text: None,
            tool_calls: vec![tool_call],
            finish: FinishReason::ToolUse,
            reasoning_content: None,
            usage: None,
        };

        let provider = Box::new(MockProvider::new(vec![tool_result.clone(), tool_result]));

        let mut harness = AgentHarness::new(config, provider, None);

        let result = harness
            .process_message("room1", "general", false, "user", "search something")
            .await;

        assert!(result.is_ok());
        let reply = result.unwrap();
        assert!(reply.is_some());
        let text = reply.unwrap();
        assert!(
            text.contains("loop"),
            "Expected loop-limit fallback, got: {}",
            text
        );
    }

    #[test]
    fn test_harness_construction() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None);
        assert_eq!(harness.memory().room_count(), 0);
        assert_eq!(harness.tools().len(), 0);
    }

    #[test]
    fn test_resolve_model() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None);
        let model = harness.resolve_model();
        assert_eq!(model, "mock-model");
    }

    #[test]
    fn test_format_messages_as_markdown() {
        let msgs = vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
        ];
        let md = format_messages_as_markdown("Test summary", &msgs);
        assert!(md.contains("Test summary"));
        assert!(md.contains("**System**: You are helpful"));
        assert!(md.contains("**User**: Hello"));
        assert!(md.contains("**Assistant**: Hi there!"));
    }

    #[test]
    fn test_format_messages_as_markdown_with_tool_calls() {
        let tc = ToolCall::new("call_1", "get_weather", r#"{"location":"Paris"}"#);
        let msgs = vec![ChatMessage::assistant_with_tool_calls("", vec![tc], None)];
        let md = format_messages_as_markdown("Summary", &msgs);
        assert!(md.contains("Summary"));
        assert!(md.contains("get_weather"));
        assert!(md.contains("Paris"));
    }

    #[tokio::test]
    async fn test_archive_room_if_needed_no_webdav() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let mut harness = AgentHarness::new(config, provider, None);

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", false);
        for i in 0..10 {
            room.history.append(ChatMessage::user(format!("msg {}", i)));
        }

        let result = harness.archive_room_if_needed("room1").await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_and_archive_returns_seq() {
        let mut mgr = MemoryManager::new(50, 12);
        let room = mgr.get_or_create("room1", "general", false);
        for i in 0..10 {
            room.history.append(ChatMessage::user(format!(
                "Message number {} with some padding text",
                i
            )));
        }

        let result = mgr.check_and_archive("room1");
        if let Some((rid, msgs, seq)) = result {
            assert_eq!(rid, "room1");
            assert!(!msgs.is_empty());
            assert_eq!(seq, 0);
        }
    }

    #[tokio::test]
    async fn test_summarize_for_archive() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None);

        let msgs = vec![
            ChatMessage::user("Hello, I need help with something"),
            ChatMessage::assistant("Sure, what do you need?"),
        ];
        let summary = harness.summarize_for_archive(&msgs).await;
        assert!(summary.starts_with("2 messages:"));
    }

    #[test]
    fn test_inject_room_context() {
        let args = r#"{"action":"read","path":"notes.txt"}"#;
        let result = inject_room_context(args, "general", "rooms/general");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["room_id"], "general");
        assert_eq!(parsed["webdav_dir"], "rooms/general");
        assert_eq!(parsed["action"], "read");
    }

    #[test]
    fn test_inject_room_context_for_image_gen() {
        let args = r#"{"prompt":"test","room_id":"x"}"#;
        let result = inject_room_context(args, "general", "rooms/general");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["room_id"], "general");
        assert_eq!(parsed["webdav_dir"], "rooms/general");
    }

    #[test]
    fn test_compute_webdav_dir_channel() {
        assert_eq!(compute_webdav_dir("atomkb", false), "rooms/atomkb");
    }

    #[test]
    fn test_compute_webdav_dir_dm() {
        assert_eq!(compute_webdav_dir("saru", true), "dms/saru");
    }
}
