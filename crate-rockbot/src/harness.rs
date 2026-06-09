use std::sync::Arc;

use tracing::{debug, error, warn};
use webdav::WebDavClient;

use crate::AppConfig;
use crate::error::Result;
use crate::memory::MemoryManager;
use crate::provider::AiProvider;
use crate::tool::ToolRegistry;
use crate::types::{ChatMessage, ChatRequest};

const MAX_AGENT_ITERATIONS: u32 = 8;

const DEFAULT_SYSTEM_PROMPT: &str = "\
You are RockBot, a helpful AI assistant running on a RocketChat server. \
You respond to DMs and @mentions concisely and helpfully. \
When you need information from the web, use the web_search tool. \
When you need to fetch a URL, use web_fetch. \
When you need to analyze an image, use the vision tool. \
Answer in the same language as the user. \
Keep responses clear and to the point.\
";

pub struct AgentHarness {
    config: Arc<AppConfig>,
    provider: Box<dyn AiProvider>,
    memory: MemoryManager,
    tools: ToolRegistry,
    webdav: Option<WebDavClient>,
}

impl AgentHarness {
    pub fn new(
        config: AppConfig,
        provider: Box<dyn AiProvider>,
        webdav: Option<WebDavClient>,
    ) -> Self {
        let max_chars = config.rocketchat.max_text_length;
        let max_history = config.rocketchat.max_history_size;
        let config = Arc::new(config);
        Self {
            config,
            provider,
            memory: MemoryManager::new(max_chars, max_history),
            tools: ToolRegistry::new(),
            webdav,
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
            if iterations > MAX_AGENT_ITERATIONS {
                warn!(
                    "Max agent iterations ({}) reached for room {}",
                    MAX_AGENT_ITERATIONS, room_id
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

                            let tool_result = self
                                .tools
                                .execute_by_name(
                                    &tool_call.function.name,
                                    &tool_call.function.arguments,
                                )
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
                &self.config.rocketchat.default_provider,
                &self.config.rocketchat.default_model,
            )
            .unwrap_or_else(|| {
                warn!(
                    "Model alias '{}' not found for provider '{}', using raw model name",
                    self.config.rocketchat.default_model, self.config.rocketchat.default_provider
                );
                self.config.rocketchat.default_model.clone()
            })
    }

    pub async fn archive_room_if_needed(&mut self, room_id: &str) -> Result<()> {
        let needs_archive = self.memory.check_and_archive(room_id);
        if let Some((rid, msgs)) = needs_archive {
            if let Some(ref _webdav) = self.webdav {
                let count = msgs.len();
                debug!("Archiving {} messages for room {}", count, rid);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RockBotError;
    use crate::provider::AiProvider;
    use crate::types::{CompletionResult, FinishReason};
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
        AppConfig::from_str(
            r#"
[rocketchat]
url = "test.example.com"
username = "bot"
password = "secret"
debug = false
default_provider = "mock"
default_model = "chat"
tools = false
max_history_size = 12
max_text_length = 50000

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
}
