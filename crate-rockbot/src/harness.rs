use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use tracing::{debug, error, info, warn};
use webdav::{WebDavClient, WebDavError, WebDavPath};
use rocketchat::RestApiClient;

use crate::AppConfig;
use crate::error::Result;
use crate::error::RockBotError;
use crate::image_cache::ImageCache;
use crate::knowledge::KnowledgeManager;
use crate::memory::{MemoryManager, SoulMemory, count_json_bytes, strip_images_from_message, strip_orphaned_tool_calls, truncate_message_content};
use crate::provider::AiProvider;
use crate::tool::ToolRegistry;
use crate::types::{ChatMessage, ChatRequest, Role};
use crate::utils::now_iso_string;

const DEFAULT_SYSTEM_PROMPT: &str = "\
You are {name}, a helpful AI assistant running on a RocketChat server. \
**Always reply in the same language as the user's most recent message.** \
Tool results, tool-call arguments, and injected image prompts may appear in \
English — ignore them when choosing your reply language; match only the \
user's language. \
You respond to DMs and @mentions concisely and helpfully. \
Context space is limited to ~{max_context_mb}MB / 1M tokens. Keep your \
reasoning brief and avoid verbose explanations. Use tools to fetch \
information rather than guessing. You have up to {max_iterations} iterations \
per task — plan your tool calls efficiently. \
Current UTC time: {current_utc_time}. Use this for all time/date questions \
and calendar calculations — do not guess or fabricate dates. \
When you need information from the web, use the web_search tool. \
When you need to fetch a URL, use web_fetch. \
When you need to describe or analyze an image, use the vision tool. \
When you need to generate or edit images, use the image_gen tool. \
Share image_gen results as markdown `![{description}]({image_key})`. \
Do NOT fabricate fake image references — only image_gen produces real images. \
When you need to read, write, list, or manage files on remote storage, use the webdav tool. \
When you need to manage calendar events or todo tasks, use the calendar tool. \
Use the edit_soul tool ONLY when the user explicitly instructs you to update your soul, \
personality, or identity (e.g. 'save this in your soul', 'update your personality', \
'remember this about yourself'). Do NOT use it for frequently changing information such as \
to-do lists, directory structures, or dynamic tasks — store those in knowledge notes or \
WebDAV files to keep the soul stable and concise. \
Before saving knowledge, ALWAYS use recall_knowledge first to check whether a related note \
already exists. If one does, update or append to the existing note instead of creating a \
duplicate. If no related note exists, you MUST ask the user for explicit permission before \
creating a new knowledge note — do NOT create new notes without user consent. Use the \
save_knowledge tool to persist entries and the forget_knowledge tool to remove them. \
When you need to recall previously saved knowledge, use the recall_knowledge tool. \
Keep responses clear and to the point.\
";

/// TTL for in-memory knowledge context cache (seconds). Knowledge is re-read from
/// WebDAV only when the cache is older than this or invalidated by a mutation.
const KNOWLEDGE_CACHE_TTL_SECS: i64 = 60;

pub struct AgentHarness {
    config: Arc<AppConfig>,
    provider: Box<dyn AiProvider>,
    memory: MemoryManager,
    tools: ToolRegistry,
    webdav: Option<WebDavClient>,
    rest_client: Option<RestApiClient>,
    max_iterations: u32,
    max_attachment_bytes: u64,
    image_pool: HashMap<String, Vec<CachedImage>>,
    image_cache: Arc<ImageCache>,
    last_image_ids: Vec<String>,
    current_image_urls: Vec<String>,
    knowledge_cache: HashMap<String, (i64, String)>,
}

impl AgentHarness {
    pub fn new(
        config: AppConfig,
        provider: Box<dyn AiProvider>,
        webdav: Option<WebDavClient>,
        image_cache: Arc<ImageCache>,
    ) -> Self {
        let active_model = config.active_model();
        let max_soul_chars = *active_model.max_soul_chars;
        let max_iterations = active_model.max_iterations;
        let persist_interval = active_model.persist_interval_secs;
        let max_context_bytes = *active_model.max_context_bytes;
        let max_attachment_bytes = active_model.max_attachment_bytes;
        let config = Arc::new(config);
        Self {
            config,
            provider,
            memory: MemoryManager::new(max_soul_chars, persist_interval, max_context_bytes),
            tools: ToolRegistry::new(),
            webdav,
            rest_client: None,
            max_iterations,
            max_attachment_bytes,
            image_pool: HashMap::new(),
            image_cache,
            last_image_ids: Vec::new(),
            current_image_urls: Vec::new(),
            knowledge_cache: HashMap::new(),
        }
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    pub fn register_tool(&mut self, tool: Box<dyn crate::tool::Tool>) {
        self.tools.register(tool);
    }

    #[cfg(test)]
    pub fn current_image_urls(&self) -> &[String] {
        &self.current_image_urls
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

    pub fn set_rest_client(&mut self, client: RestApiClient) {
        self.rest_client = Some(client);
    }

    pub fn has_rest_client(&self) -> bool {
        self.rest_client.is_some()
    }

    pub fn take_last_image_ids(&mut self) -> Vec<String> {
        std::mem::take(&mut self.last_image_ids)
    }

    pub fn take_image(&self, key: &str) -> Option<crate::image_cache::GeneratedImage> {
        self.image_cache.take(key)
    }

    pub async fn resolve_room_fname(&mut self, room_id: &str) -> Option<String> {
        if let Some(ref mut client) = self.rest_client {
            client.resolve_room_fname(room_id).await
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn process_message(
        &mut self,
        room_id: &str,
        room_name: &str,
        room_fname: &str,
        is_dm: bool,
        sender_name: &str,
        text: &str,
        attachments: &[rocketchat::AttachmentInfo],
        msg_urls: &[rocketchat::MessageUrl],
    ) -> Result<Option<String>> {
        let msg_start = std::time::Instant::now();

        // Collect image URLs from the message's url field for automatic
        // injection into image_gen calls (bypasses vision for text-only models)
        self.current_image_urls = msg_urls
            .iter()
            .filter(|u| u.headers.as_ref().and_then(|h| h.content_type.as_deref())
                .is_some_and(|ct| ct.starts_with("image/")))
            .map(|u| u.url.clone())
            .collect();

        let clean_text = if !is_dm && !text.is_empty() {
            text.trim_start().to_string()
        } else {
            text.to_string()
        };

        let needs_restore = {
            let room = self.memory.get_or_create(room_id, room_name, room_fname, is_dm);
            room.touch();
            room.history.messages.is_empty() && room.history.archive_seq == 0
        };

        if needs_restore && self.webdav.is_some() {
            self.restore_history(room_id, room_name, room_fname, is_dm).await;
        }

        let user_text = format!("{}: {}", sender_name, clean_text);

        // Download all image attachments and encode as data URIs,
        // paired with their filenames for markdown-based referencing.
        let attachment_refs = self.download_attachment_refs(attachments).await;

        let user_msg = if !attachment_refs.is_empty() {
            let data_uris: Vec<String> = attachment_refs.iter().map(|r| r.data_uri.clone()).collect();
            let image_labels: String = attachment_refs
                .iter()
                .map(|r| format!("![{}]({})", r.title, r.title))
                .collect::<Vec<_>>()
                .join(" ");
            let prompt = if clean_text.is_empty() {
                format!("{}:\nAttached: {}", sender_name, image_labels)
            } else {
                format!(
                    "{}: {}\nAttached: {}",
                    sender_name, clean_text, image_labels
                )
            };
            ChatMessage::user_with_images(prompt, data_uris)
        } else {
            ChatMessage::user(user_text)
        };

        if let Some(room) = self.memory.get_mut(room_id) {
            room.history.append(user_msg);
        }

        self.memory.mark_snapshot_dirty(room_id);

        let wd = compute_webdav_dir(room_name, room_fname, is_dm);
        let secrets = load_secrets_from_webdav(self.webdav.as_ref(), &wd).await;
        let system_prompt = match &secrets {
            Some(s) => self.build_system_prompt_with_secrets(s),
            None => self.build_system_prompt(),
        };
        let tool_defs = self.tools.definitions();
        let have_tools = !tool_defs.is_empty();

        let model = self.resolve_model();

        if let Err(e) = self.refresh_knowledge_context(room_id, &wd).await {
            warn!("Failed to refresh knowledge context: {}", e);
        }

        let mut messages = self
            .memory
            .build_context(room_id, &system_prompt, None, None);
        // Inline context trim: reduce if approaching byte limit (no LLM call)
        let max_ctx = *self.config.active_model().max_context_bytes as u64;
        let before_trim = messages.len();
        messages = self.trim_context(room_id, messages, max_ctx).await;
        if messages.len() < before_trim {
            self.memory.set_byte_pressure(room_id);
        }
        strip_orphaned_tool_calls(&mut messages);
        debug!(
            "Built context for room {}: {} messages (model={}, have_tools={})",
            room_id,
            messages.len(),
            model,
            have_tools,
        );

        let mut iterations: u32 = 0;
        let mut image_ids_this_turn: Vec<String> = Vec::new();
        let mut context_compressed = false;

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
                debug!(
                    "process_message max_iterations reached: total_elapsed_ms={}",
                    msg_start.elapsed().as_millis(),
                );
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

            let llm_start = std::time::Instant::now();
            match self.provider.complete(request).await {
                Ok(result) => {
                    debug!(
                        "LLM call completed in {}ms (iteration {}/{}, tool_calls={}, has_text={})",
                        llm_start.elapsed().as_millis(),
                        iterations,
                        self.max_iterations,
                        result.tool_calls.len(),
                        result.text.is_some(),
                    );

                    // Check token pressure: if usage nears model context limit, flag for background compression
                    if let Some(ref usage) = result.usage {
                        let threshold = (self.config.active_model().model_context_length as f64 * 0.9) as u64;
                        if usage.total_tokens > threshold {
                            debug!(
                                "Token pressure detected: {} total_tokens > 90% of {} (threshold={})",
                                usage.total_tokens, self.config.active_model().model_context_length, threshold
                            );
                            self.memory.set_token_pressure(room_id);
                        }
                    }

                    if !result.tool_calls.is_empty() {
                        let assistant_msg = ChatMessage::assistant_with_tool_calls(
                            "",
                            result.tool_calls.clone(),
                            result.reasoning_content.clone(),
                        );
                        self.append_to_history(room_id, assistant_msg);

                        let mut altered_soul = false;
                        let mut altered_knowledge = false;

                        for tool_call in &result.tool_calls {
                            debug!(
                                "Executing tool {} (call_id: {})",
                                tool_call.function.name, tool_call.id
                            );

                            let tool_start = std::time::Instant::now();
                            if tool_call.function.name == "edit_soul" {
                                altered_soul = true;
                            }
                            if tool_call.function.name == "save_knowledge"
                                || tool_call.function.name == "forget_knowledge"
                            {
                                altered_knowledge = true;
                            }

                            let arguments = if tool_call.function.name == "image_gen" {
                                let wd = compute_webdav_dir(room_name, room_fname, is_dm);
                                let mut args = inject_image_urls_from_refs(
                                    &tool_call.function.arguments,
                                    room_id,
                                    &wd,
                                    &attachment_refs,
                                    Some(&self.image_pool),
                                );
                                if let Ok(mut v) =
                                    serde_json::from_str::<serde_json::Value>(&args)
                                {
                                    if let Some(obj) = v.as_object_mut() {
                                        obj.insert(
                                            "image_cache_key".to_string(),
                                            serde_json::Value::String(
                                                tool_call.id.clone(),
                                            ),
                                        );
                                        // Auto-inject image URLs from the current message
                                        // (e.g. NextCloud share links) so text-only
                                        // models can edit images without needing vision.
                                        if !self.current_image_urls.is_empty() {
                                            debug!("Auto-injecting {} message image URL(s) into image_gen args",
                                                self.current_image_urls.len());
                                            let existing = obj
                                                .get("image_urls")
                                                .and_then(|v| v.as_array())
                                                .map(|arr| {
                                                    arr.iter()
                                                        .filter_map(|v| v.as_str().map(String::from))
                                                        .collect::<Vec<_>>()
                                                })
                                                .unwrap_or_default();
                                            let mut urls: Vec<serde_json::Value> = existing
                                                .iter()
                                                .map(|s| serde_json::Value::String(s.clone()))
                                                .collect();
                                            for url in &self.current_image_urls {
                                                let s = serde_json::Value::String(url.clone());
                                                if !urls.contains(&s) {
                                                    urls.push(s);
                                                }
                                            }
                                            if !urls.is_empty() {
                                                obj.insert(
                                                    "image_urls".to_string(),
                                                    serde_json::Value::Array(urls),
                                                );
                                            }
                                        }
                                    }
                                    args = serde_json::to_string(&v).unwrap_or(args);
                                }
                                args
                            } else if tool_call.function.name == "web_fetch" {
                                match &secrets {
                                    Some(entries) if !entries.is_empty() => {
                                        debug!("Intercepting web_fetch call {} — secret resolution active ({} entries loaded)",
                                            tool_call.id, entries.len());
                                        let host_map = filter_secrets_by_host(entries, &tool_call.function.arguments);
                                        match host_map {
                                            Some(ref map) if !map.is_empty() => {
                                                let resolved = resolve_secret_refs_deep(&tool_call.function.arguments, map);
                                                debug!("Secret resolution complete for web_fetch call {}", tool_call.id);
                                                resolved
                                            }
                                            _ => {
                                                debug!("No matching secrets for web_fetch call {} — passing arguments through", tool_call.id);
                                                tool_call.function.arguments.clone()
                                            }
                                        }
                                    }
                                    _ => {
                                        debug!("No secrets loaded — passing web_fetch call {} through unchanged", tool_call.id);
                                        tool_call.function.arguments.clone()
                                    }
                                }
                            } else if tool_call.function.name == "webdav"
                                || tool_call.function.name == "edit_soul"
                                || tool_call.function.name == "save_knowledge"
                                || tool_call.function.name == "forget_knowledge"
                                || tool_call.function.name == "recall_knowledge"
                                || tool_call.function.name == "calendar"
                                || tool_call.function.name == "compress_memory"
                            {
                                let wd = compute_webdav_dir(room_name, room_fname, is_dm);
                                inject_room_context(&tool_call.function.arguments, room_id, &wd)
                            } else {
                                tool_call.function.arguments.clone()
                            };

                            // compress_memory sets a flag for post-reply
                            // execution. Running compression now would clear
                            // the conversation history the LLM needs to
                            // generate a coherent reply.
                            let tool_result = if tool_call.function.name == "compress_memory" {
                                let call_id = crate::validated::NonEmptyString::try_new(tool_call.id.clone())
                                    .expect("non-empty tool call id from provider");
                                self.memory.set_explicit_compress(room_id);
                                crate::tool::ToolResult {
                                    call_id,
                                    name: crate::validated::NonEmptyString::try_new("compress_memory".to_string())
                                        .expect("non-empty tool name"),
                                    is_error: false,
                                    content: "Memory compression scheduled. Reply to the user first — compression will execute after your reply is sent.".to_string(),
                                }
                            } else {
                                self.tools
                                    .execute_by_name(&tool_call.id, &tool_call.function.name, &arguments)
                                    .await
                                    .unwrap_or_else(|e| {
                                        crate::tool::ToolResult {
                                            call_id: crate::validated::NonEmptyString::try_new(tool_call.id.clone()).expect("non-empty tool call id from provider"),
                                            name: crate::validated::NonEmptyString::try_new(tool_call.function.name.clone()).expect("non-empty tool name from provider"),
                                            is_error: true,
                                            content: format!("Tool error: {}", e),
                                        }
                                    })
                            };

                            debug!(
                                "Tool {} completed in {}ms (is_error={})",
                                tool_call.function.name,
                                tool_start.elapsed().as_millis(),
                                tool_result.is_error,
                            );

                            if tool_call.function.name == "image_gen" && !tool_result.is_error {
                                debug!("image_gen call {} completed — queuing for post-reply upload", tool_call.id);
                                image_ids_this_turn.push(tool_call.id.clone());
                                // Add generated image to image_pool for name-based
                                // matching in subsequent tool calls (e.g. "make the
                                // fluffy cat darker" matches a prior image_gen prompt)
                                if let Some(cached) = self.image_cache.get(&tool_call.id) {
                                    if let Ok(v) =
                                        serde_json::from_str::<serde_json::Value>(
                                            &tool_call.function.arguments,
                                        )
                                    {
                                        if let Some(prompt) =
                                            v.get("prompt").and_then(|p| p.as_str())
                                        {
                                            let name = if prompt.len() > 80 {
                                                format!("{}...", &prompt[..77])
                                            } else {
                                                prompt.to_string()
                                            };
                                            self.image_pool
                                                .entry(room_id.to_string())
                                                .or_default()
                                                .push(CachedImage {
                                                    data_uri: cached.data_uri(),
                                                    name: name.clone(),
                                                });
                                            debug!(
                                                "Added generated image to image_pool for room {}: {}",
                                                room_id, name,
                                            );
                                        }
                                    }
                                }
                            }

                            let tool_msg = ChatMessage::tool(&tool_call.id, &tool_result.content);
                            self.append_to_history(room_id, tool_msg);
                        }

                        if altered_soul {
                            if let Some(ref webdav_client) = self.webdav {
                                let wd = compute_webdav_dir(room_name, room_fname, is_dm);
                                if let Ok(soul) = self.load_soul(webdav_client, &wd).await {
                                    self.memory.set_soul(room_id, soul);
                                }
                            }
                            self.memory.mark_snapshot_dirty(room_id);
                        }
                        if altered_knowledge {
                            self.memory.mark_snapshot_dirty(room_id);
                            let wd = compute_webdav_dir(room_name, room_fname, is_dm);
                            self.knowledge_cache.remove(&wd);
                            if let Err(e) = self.refresh_knowledge_context(room_id, &wd).await {
                                warn!("Failed to refresh knowledge context after alter: {}", e);
                            }
                        }

                        messages = self
                            .memory
                            .build_context(room_id, &system_prompt, None, None);
                        let max_ctx = *self.config.active_model().max_context_bytes as u64;
                        let before_trim2 = messages.len();
                        messages = self.trim_context(room_id, messages, max_ctx).await;
                        if messages.len() < before_trim2 {
                            self.memory.set_byte_pressure(room_id);
                        }
                        strip_orphaned_tool_calls(&mut messages);
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
                        debug!(
                            "process_message done: total_elapsed_ms={} iterations={}",
                            msg_start.elapsed().as_millis(),
                            iterations,
                        );
                        self.last_image_ids = image_ids_this_turn;
                        if !self.last_image_ids.is_empty() {
                            debug!("Carrying {} generated image(s) for post-reply upload: {:?}",
                                self.last_image_ids.len(), self.last_image_ids);
                        }
                        return Ok(Some(reply));
                    }

                    let fallback = "I received a response but it was empty. Please try again.";
                    let assistant_msg = ChatMessage::assistant(fallback);
                    self.append_to_history(room_id, assistant_msg);
                    debug!(
                        "process_message empty response fallback: total_elapsed_ms={}",
                        msg_start.elapsed().as_millis(),
                    );
                    return Ok(Some(fallback.to_string()));
                }
                Err(e) => {
                    if matches!(e, RockBotError::ContextLengthExceeded(_)) {
                        if !context_compressed {
                            warn!(
                                "Context length exceeded for room {}, compressing memory and retrying",
                                room_id
                            );
                            let _ = self.compress_room_inner(room_id, false).await;
                            context_compressed = true;
                            // Rebuild with minimal history then hard-truncate
                            messages = self
                                .memory
                                .build_context(room_id, &system_prompt, Some(4), None);
                            // Hard truncation: keep system/front-matter messages
                            // at the start, and only the last 2 conversation
                            // messages at the end to guarantee token fit.
                            let system_end = messages
                                .iter()
                                .position(|m| m.role != Role::System)
                                .unwrap_or(1);
                            let keep_last = 2usize;
                            if messages.len() > system_end + keep_last {
                                let drop = messages.len() - system_end - keep_last;
                                messages.drain(system_end..system_end + drop);
                            }
                            // Emergency per-message content truncation to
                            // handle cases where remaining messages contain
                            // enormous text (large tool results, pastes, etc.)
                            for msg in messages.iter_mut().skip(system_end) {
                                truncate_message_content(msg, 200_000);
                            }
                            strip_orphaned_tool_calls(&mut messages);
                            debug!(
                                "Context length retry for room {}: hard-truncated to {} messages",
                                room_id, messages.len()
                            );
                            continue;
                        }
                        warn!(
                            "Context length exceeded again after compression for room {}, giving up",
                            room_id
                        );
                    }
                    error!("AI provider error: {}", e);
                    let fallback = format!("I encountered an error: {}. Please try again.", e);
                    let assistant_msg = ChatMessage::assistant(&fallback);
                    self.append_to_history(room_id, assistant_msg);
                    debug!(
                        "process_message provider error: total_elapsed_ms={}",
                        msg_start.elapsed().as_millis(),
                    );
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
        let name = &self.config.rocketchat.server.username;
        let max_ctx = *self.config.active_model().max_context_bytes as f64 / 1_000_000.0;
        let max_iter = self.config.active_model().max_iterations;
        let utc_time = crate::utils::now_utc_human();
        DEFAULT_SYSTEM_PROMPT
            .replace("{name}", name)
            .replace("{max_context_mb}", &format!("{max_ctx:.1}"))
            .replace("{max_iterations}", &max_iter.to_string())
            .replace("{current_utc_time}", &utc_time)
    }

    fn build_system_prompt_with_secrets(&self, secrets: &[ResolvedSecret]) -> String {
        let base = self.build_system_prompt();
        let uuid_prompt = build_secret_uuids_prompt(secrets);
        format!("{base}{uuid_prompt}")
    }

    fn resolve_model(&self) -> String {
        self.config
            .resolve_chat_model(
            self.config.active_model().default_provider.as_str(),
            &self.config.active_model().default_model,
            )
            .unwrap_or_else(|| {
                warn!(
                    "Model alias '{}' not found for provider '{}', using raw model name",
                    self.config.active_model().default_model,
                    self.config.active_model().default_provider.as_str()
                );
                self.config.active_model().default_model.clone()
            })
    }

    async fn download_attachment_refs(
        &self,
        attachments: &[rocketchat::AttachmentInfo],
    ) -> Vec<AttachmentRef> {
        let mut refs = Vec::with_capacity(attachments.len());
        for att in attachments {
            let title_link = match att.title_link.as_deref() {
                Some(link) if !link.is_empty() => link,
                _ => continue,
            };
            let title = att
                .title
                .as_deref()
                .filter(|t| !t.is_empty())
                .unwrap_or("image")
                .to_string();
            // Pre-encoded data: URI from platform layer (e.g. Matrix m.image handler).
            // Pass through directly without HTTP fetch.
            if title_link.starts_with("data:") {
                refs.push(AttachmentRef {
                    title,
                    data_uri: title_link.to_string(),
                });
                continue;
            }
            let host = self.config.rocketchat.server.url
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/');
            let url = format!("https://{}{}", host, title_link);
            match self.download_and_encode_single(&url).await {
                Ok(data_uri) => refs.push(AttachmentRef { title, data_uri }),
                Err(e) => warn!("Failed to download attachment {}: {}", url, e),
            }
        }
        refs
    }

    async fn download_and_encode_single(&self, url: &str) -> Result<String> {
        let mut req = self.provider_http_client().get(url);
        if let Some(ref rest) = self.rest_client {
            req = req.headers(rest.headers());
        }
        let response = req
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| crate::error::RockBotError::Provider(format!("Attachment download failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(crate::error::RockBotError::Provider(format!(
                "Attachment download HTTP {}",
                status
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let mime = content_type.as_deref().unwrap_or("image/png");

        if let Some(len) = response.content_length() {
            if len > self.max_attachment_bytes {
                return Err(crate::error::RockBotError::Provider(format!(
                    "Attachment too large: {} bytes exceeds {} byte limit",
                    len, self.max_attachment_bytes
                )));
            }
        }

        let bytes = response.bytes().await.map_err(|e| {
            crate::error::RockBotError::Provider(format!("Attachment read failed: {e}"))
        })?;

        Ok(format!(
            "data:{};base64,{}",
            mime,
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        ))
    }

    fn provider_http_client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }

    /// Fast in-memory safety net: strips images from oldest messages, keeps
    /// system prefix + last 2 messages. Sets byte_pressure_flag so the room
    /// gets full LLM compression after reply delivery. No LLM call, no delay.
    async fn trim_context(
        &self,
        room_id: &str,
        messages: Vec<ChatMessage>,
        max_bytes: u64,
    ) -> Vec<ChatMessage> {
        let current_bytes: u64 = messages
            .iter()
            .map(|m| count_json_bytes(m) as u64)
            .sum();
        if current_bytes <= max_bytes {
            return messages;
        }

        let system_idx = messages.iter().position(|m| m.role == Role::System);
        let start = system_idx.map(|i| i + 1).unwrap_or(0);
        if messages.len() <= start + 4 {
            return messages;
        }

        let prefix = if let Some(idx) = system_idx {
            messages[..=idx].to_vec()
        } else {
            vec![]
        };

        // Keep last 2 conversation messages, strip images from earlier ones
        let suffix_start = (messages.len() - start).saturating_sub(2);
        let suffix = if suffix_start > 0 && start + suffix_start < messages.len() {
            let mut trimmed: Vec<ChatMessage> = messages[start + suffix_start..]
                .iter()
                .map(|m| strip_images_from_message(m.clone()))
                .collect();
            // Preserve images on the last message (current user request)
            if let Some(last) = trimmed.last_mut() {
                if let Some(orig_last) = messages.last().cloned() {
                    *last = orig_last;
                }
            }
            trimmed
        } else {
            messages[start + messages.len().saturating_sub(2).min(messages.len())..].to_vec()
        };

        let mut result = prefix;
        result.extend(suffix);

        debug!(
            "trim_context for room {}: {} messages -> {} ({} -> {} bytes), byte_pressure_flag set",
            room_id,
            messages.len(),
            result.len(),
            current_bytes,
            result.iter()
                .map(|m| count_json_bytes(m))
                .sum::<usize>(),
        );

        // Set flag for background compression after reply
        // Access via self.memory (need mutable reference)
        result
    }

    pub async fn compress_room_if_needed(&mut self, room_id: &str) -> Result<()> {
        let needs_compress = self.memory.needs_compression(room_id);
        if !needs_compress {
            return Ok(());
        }
        // explicit_compress flag triggers force=true (full compression, not half)
        let force = self.memory.has_explicit_compress(room_id);
        self.compress_room_inner(room_id, force).await
    }

    /// Force-compress all Layer 1 messages (user-triggered, synchronous)
    pub async fn compress_room_full(&mut self, room_id: &str) -> Result<String> {
        self.compress_room_inner(room_id, true).await?;
        let summary = self.memory.get_summary(room_id).unwrap_or("").to_string();
        Ok(format!("Memory compressed. Summary:\n\n{}", summary))
    }

    async fn compress_room_inner(&mut self, room_id: &str, force: bool) -> Result<()> {
        let needs_compress = self.memory.check_and_archive(room_id, force);
        if let Some((rid, msgs)) = needs_compress {
            if let Some(ref webdav_client) = self.webdav {
                let count = msgs.len();

                let wd = {
                    let room = self.memory.get(&rid);
                    let (rn, rf, dm) = room
                        .map(|r| (r.room_name.as_str(), r.room_fname.as_str(), r.is_dm))
                        .unwrap_or((&rid, "", false));
                    compute_webdav_dir(rn, rf, dm)
                };

                // Load existing summary.md and knowledge entries
                let existing_summary = {
                    let path = self.memory.summary_path(&wd);
                    webdav_client.read_file_to_string(&path).await.ok()
                };

                // Load knowledge index to give LLM context
                let knowledge_entries = match crate::knowledge::KnowledgeManager::load_index(webdav_client, &wd).await {
                    Ok(idx) => idx.entries,
                    Err(_) => Vec::new(),
                };

                // LLM compress: summary + identify used knowledge
                let (summary_text, used_filenames) = self.compress_for_summary(&msgs, existing_summary.as_deref(), &knowledge_entries).await;

                // Write summary.md
                let summary_ok = self.write_summary_md(webdav_client, &wd, &summary_text).await.is_ok();
                if !summary_ok {
                    warn!("Failed to write summary.md, skipping prune");
                }

                if summary_ok {
                    self.memory.mark_snapshot_dirty(&rid);

                    // Review knowledge priorities with LLM-identified used entries
                    if let Ok(changed) = crate::knowledge::KnowledgeManager::review_priorities(
                        webdav_client, &wd, &used_filenames,
                    ).await {
                        if changed {
                            if let Ok(text) = self.load_knowledge_for_room(webdav_client, &rid, &wd).await {
                                if !text.is_empty() {
                                    self.memory.set_knowledge(&rid, text);
                                }
                            }
                            self.memory.mark_snapshot_dirty(&rid);
                        }
                    }

                    // Update in-memory summary cache
                    self.memory.set_summary(&rid, Some(summary_text));

                    // Clear pressure flags after compression
                    self.memory.prune_archived(&rid, count);
                    self.memory.clear_pressure_flags(&rid);
                }
            } else {
                debug!(
                    "No WebDAV client, truncating instead of compressing for room {}",
                    rid
                );
                let count = msgs.len();
                self.memory.prune_archived(&rid, count);
                self.memory.clear_pressure_flags(&rid);
            }
        }
        Ok(())
    }

    async fn compress_for_summary(
        &self,
        messages: &[ChatMessage],
        existing_summary: Option<&str>,
        knowledge_entries: &[crate::knowledge::IndexEntry],
    ) -> (String, Vec<String>) {
        if messages.is_empty() {
            return (String::new(), Vec::new());
        }

        let user_msgs: Vec<String> = messages
            .iter()
            .filter(|m| m.role == Role::User || m.role == Role::Assistant)
            .filter_map(|m| m.text_content())
            .map(|t| t.chars().take(300).collect::<String>())
            .take(20)
            .collect();

        if user_msgs.is_empty() {
            return (format!("{} messages compressed", messages.len()), Vec::new());
        }

        // Build knowledge entries reference for LLM
        let mut knowledge_ref = String::new();
        if !knowledge_entries.is_empty() {
            knowledge_ref.push_str("\n## Knowledge Entries (identify which were relevant)\n");
            for entry in knowledge_entries.iter().take(30) {
                knowledge_ref.push_str(&format!(
                    "- `{}` — {}\n",
                    entry.filename,
                    entry.when_useful
                ));
            }
        }

        let existing_block = existing_summary
            .filter(|s| !s.is_empty())
            .map(|s| format!("\n## Existing Summary\n{}", s))
            .unwrap_or_default();

        let prompt = format!(
            "Compress this conversation excerpt into at most 10 bullet points for a memory summary.\n\
             Focus on key facts, decisions, user preferences, and persistent information.\n\
             Output format:\n\
             # Memory Summary\n\n\
             - bullet point 1\n\
             - bullet point 2\n\
             ...\n\n\
             ## Used Knowledge\n\
             - filename1.md\n\
             - filename2.md\n\n\
             Only list knowledge entries that were actually relevant to this conversation.\n\
             {}\n\
             ## Conversation\n{}\n",
            existing_block,
            user_msgs.join("\n")
        );

        let request = ChatRequest {
            model: self.resolve_model(),
            messages: vec![ChatMessage::user(&prompt)],
            tools: None,
            stream: false,
            temperature: Some(0.3),
            max_tokens: Some(512),
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let default_summary = format!("{} messages compressed", messages.len());

        match self.provider.complete(request).await {
            Ok(result) => {
                let text = result.text.unwrap_or_else(|| default_summary.clone());
                debug!("compress_for_summary raw AI response text ({} chars):\n{}", text.len(), &text[..text.len().min(1000)]);
                if let Some(ref rc) = result.reasoning_content {
                    debug!("compress_for_summary reasoning_content ({} chars):\n{}", rc.len(), &rc[..rc.len().min(500)]);
                }
                parse_compression_output(&text, &default_summary)
            }
            Err(e) => {
                warn!("AI compression failed, using fallback: {}", e);
                (default_summary, Vec::new())
            }
        }
    }

    async fn write_summary_md(
        &self,
        webdav: &WebDavClient,
        webdav_dir: &str,
        summary: &str,
    ) -> Result<()> {
        let path = self.memory.summary_path(webdav_dir);
        let folder = format!("{}memory/", WebDavPath::new("").room_dir(webdav_dir));
        if let Err(e) = webdav.ensure_directory_all(&folder).await {
            warn!("Failed to ensure memory directory {}: {}", folder, e);
        }
        webdav
            .write_file_with_fallback(&path, summary.as_bytes().to_vec())
            .await
            .map_err(|e| crate::error::RockBotError::Provider(format!("summary.md write failed: {e}")))?;
        debug!("Wrote summary.md at {}", path);
        Ok(())
    }

    pub async fn restore_history(
        &mut self,
        room_id: &str,
        room_name: &str,
        room_fname: &str,
        is_dm: bool,
    ) {
        let wd = compute_webdav_dir(room_name, room_fname, is_dm);

        let webdav_client = match &self.webdav {
            Some(c) => c,
            None => return,
        };

        // Cache-first: try snapshot.json for single-read restore
        let snap_path = format!("{}memory/snapshot.json", WebDavPath::new("").room_dir(&wd));
        let mut got_soul = false;
        let mut got_summary = false;

        if let Ok(content) = webdav_client.read_file_to_string(&snap_path).await {
            if let Ok(snapshot) = serde_json::from_str::<crate::memory::PersistSnapshot>(&content) {
                // Schema version check: reject unknown schemas
                if snapshot.schema.as_str() == "rockbot-snapshot/1" {
                    self.memory.restore_snapshot(&snapshot);
                    got_soul = snapshot.soul.is_some();
                    got_summary = snapshot.summary.is_some();
                    debug!(
                        "Restored snapshot for room {} (soul={}, summary={})",
                        room_name, got_soul, got_summary
                    );
                } else {
                    warn!(
                        "Unknown snapshot schema '{}' for room {}, using individual files",
                        snapshot.schema.as_str(), room_name
                    );
                }
            }
        }

        // Fallback: load individual files for any missing layers
        if !got_summary {
            match self.load_summary(webdav_client, &wd).await {
                Ok(Some(summary)) => {
                    debug!("Loaded summary.md for room {}", room_name);
                    self.memory.set_summary(room_id, Some(summary));
                }
                Ok(None) => {
                    debug!("No summary.md found for room {}", room_name);
                }
                Err(e) => {
                    warn!(
                        "Failed to load summary.md for room {}: {}",
                        room_name, e
                    );
                }
            }
        }

        if !got_soul {
            // Layer 3: load soul.md from WebDAV
            match self.load_soul(webdav_client, &wd).await {
                Ok(soul) => {
                    if !soul.content.is_empty() {
                        debug!("Loaded soul.md for room {}", room_name);
                    }
                    self.memory.set_soul(room_id, soul);
                }
                Err(e) => {
                    warn!(
                        "Failed to load soul.md for room {}: {}",
                        room_name, e
                    );
                }
            }
        }

        // Knowledge: load index and match against context
        match self.load_knowledge_for_room(webdav_client, room_id, &wd).await {
            Ok(text) => {
                if !text.is_empty() {
                    self.memory.set_knowledge(room_id, text);
                }
                debug!("Loaded knowledge context for room {}", room_name);
            }
            Err(e) => {
                warn!(
                    "Failed to load knowledge for room {}: {}",
                    room_name, e
                );
            }
        }
    }

    async fn load_summary(
        &self,
        webdav: &WebDavClient,
        webdav_dir: &str,
    ) -> Result<Option<String>> {
        let path = self.memory.summary_path(webdav_dir);
        match webdav.read_file_to_string(&path).await {
            Ok(content) => {
                if content.trim().is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(content))
                }
            }
            Err(e) => {
                debug!("No summary.md at {} yet: {}", path, e);
                Ok(None)
            }
        }
    }

    async fn load_soul(
        &self,
        webdav: &WebDavClient,
        webdav_dir: &str,
    ) -> Result<SoulMemory> {
        let path = format!("{}memory/soul.md", WebDavPath::new("").room_dir(webdav_dir));
        let content = match webdav.read_file_to_string(&path).await {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to load soul.md from {}: {} — returning empty soul", path, e);
                return Ok(SoulMemory {
                    room_id: crate::validated::NonEmptyString::try_new(webdav_dir.to_string()).expect("webdav_dir must be non-empty"),
                    content: String::new(),
                    updated_at: String::new(),
                });
            }
        };

        let updated_at = now_iso_string();

        Ok(SoulMemory {
            room_id: crate::validated::NonEmptyString::try_new(webdav_dir.to_string()).expect("webdav_dir must be non-empty"),
            content,
            updated_at,
        })
    }

    async fn load_knowledge_for_room(
        &self,
        webdav: &WebDavClient,
        room_id: &str,
        webdav_dir: &str,
    ) -> Result<String> {
        let index = KnowledgeManager::load_index(webdav, webdav_dir).await?;
        if index.entries.is_empty() {
            return Ok(String::new());
        }

        let recent: Vec<String> = {
            self.memory
                .get(room_id)
                .map(|r| {
                    r.history
                        .messages
                        .iter()
                        .filter(|m| m.role == crate::types::Role::User)
                        .filter_map(|m| m.text_content())
                        .map(|t| t.to_string())
                        .rev()
                        .take(10)
                        .collect()
                })
                .unwrap_or_default()
        };

        let recent_refs: Vec<&str> = recent.iter().map(|s| s.as_str()).collect();
        let matching = KnowledgeManager::match_relevant(&index, &recent_refs);

        let mut parts = Vec::new();
        for entry in &matching {
            let md_path = format!(
                "{}knowledge/{}",
                WebDavPath::new("").room_dir(webdav_dir),
                entry.filename
            );
            match webdav.read_file_to_string(&md_path).await {
                Ok(body) => {
                    parts.push(format!(
                        "[Knowledge: {}]\n{}",
                        entry.display_title(), body
                    ));
                }
                Err(e) => {
                    warn!("Failed to read knowledge entry {}: {}", entry.filename, e);
                }
            }
        }

        if parts.is_empty() {
            return Ok(String::new());
        }

        Ok(format!(
            "[Knowledge — automatically recalled for this conversation]\n{}",
            parts.join("\n---\n")
        ))
    }

    pub async fn refresh_knowledge_context(
        &mut self,
        room_id: &str,
        webdav_dir: &str,
    ) -> Result<()> {
        // Check in-memory cache: if fresh (< TTL) and present, skip WebDAV reads.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        if let Some((cached_ts, cached_text)) = self.knowledge_cache.get(webdav_dir) {
            if now - cached_ts < KNOWLEDGE_CACHE_TTL_SECS {
                if !cached_text.is_empty() {
                    self.memory.set_knowledge(room_id, cached_text.clone());
                }
                return Ok(());
            }
        }

        let webdav = self.webdav.clone();
        if let Some(ref webdav) = webdav {
            let text = self
                .load_knowledge_for_room(webdav, room_id, webdav_dir)
                .await?;
            self.knowledge_cache.insert(webdav_dir.to_string(), (now, text.clone()));
            if !text.is_empty() {
                self.memory.set_knowledge(room_id, text);
            }
        }
        Ok(())
    }

    /// Persist all dirty snapshots to WebDAV immediately (used for graceful shutdown).
    pub async fn flush_all_snapshots(&mut self) {
        let webdav_client = match &self.webdav {
            Some(c) => c,
            None => return,
        };

        let dirty: Vec<String> = self.memory.dirty_snapshots();
        for room_id in &dirty {
            let snapshot = match self.memory.build_snapshot(room_id) {
                Some(s) => s,
                None => {
                    self.memory.clear_dirty(room_id);
                    continue;
                }
            };

            if snapshot.messages.is_empty()
                && snapshot.soul.is_none()
                && snapshot.summary.is_none()
            {
                self.memory.clear_dirty(room_id);
                continue;
            }

            let wd = {
                let room = self.memory.get(room_id);
                room.map(|r| compute_webdav_dir(&r.room_name, &r.room_fname, r.is_dm))
                    .unwrap_or_default()
            };

            if wd.is_empty() {
                self.memory.clear_dirty(room_id);
                continue;
            }

            let path = format!("{}memory/snapshot.json", WebDavPath::new("").room_dir(&wd));
            let json = match serde_json::to_vec(&snapshot) {
                Ok(j) => j,
                Err(e) => {
                    warn!("Failed to serialize snapshot for {}: {}", room_id, e);
                    continue;
                }
            };

            match webdav_client.write_file_with_fallback(&path, json).await {
                Ok(()) => {
                    self.memory.clear_dirty(room_id);
                    info!("Flushed snapshot for room {}", room_id);
                }
                Err(e) => {
                    warn!(
                        "Failed to flush snapshot for {}: {}",
                        room_id, e
                    );
                }
            }
        }
    }

    pub async fn maintenance_tick(&mut self, memory_ttl_secs: u64) {
        // Phase 1: persist dirty snapshots
        if self.webdav.is_some() {
            let dirty_count = self.memory.dirty_snapshots().len();
            if dirty_count > 0 {
                debug!("maintenance_tick: flushing {} dirty snapshot(s)", dirty_count);
            }
            self.flush_all_snapshots().await;
        }

        // Phase 2: evict stale rooms
        let stale: Vec<String> = self.memory.stale_rooms(memory_ttl_secs);
        for room_id in &stale {
            let room_name = self
                .memory
                .get(room_id)
                .map(|r| r.room_name.clone())
                .unwrap_or_default();
            debug!("Evicting stale room {} ({})", room_name, room_id);
            self.memory.evict_room(room_id);
        }
    }
}

struct AttachmentRef {
    pub title: String,
    pub data_uri: String,
}

struct CachedImage {
    data_uri: String,
    name: String,
}

#[derive(Debug, serde::Deserialize)]
struct SecretEntry {
    host: String,
    key: String,
    value: String,
}

#[derive(Debug, Clone)]
struct ResolvedSecret {
    uuid: String,
    host: String,
    key: String,
    value: String,
}

#[derive(Debug, serde::Deserialize)]
struct SecretsToml {
    #[serde(default)]
    secrets: Vec<SecretEntry>,
}

const SECRET_UUID_NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1,
    0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

fn generate_secret_uuid(host: &str, key: &str) -> String {
    let name = format!("{host}:{key}");
    uuid::Uuid::new_v5(&SECRET_UUID_NAMESPACE, name.as_bytes()).to_string()
}

fn build_secret_uuids_prompt(secrets: &[ResolvedSecret]) -> String {
    if secrets.is_empty() {
        return String::new();
    }
    let mut lines = vec!["\nAvailable API secrets (use secret:<UUID> to authenticate):".to_string()];
    for s in secrets {
        lines.push(format!("- secret:{} ({})", s.uuid, s.key));
    }
    lines.join("\n")
}

fn inject_room_context(arguments: &str, room_id: &str, webdav_dir: &str) -> String {
    let mut args: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));
    args["room_id"] = serde_json::Value::String(room_id.to_string());
    args["webdav_dir"] = serde_json::Value::String(webdav_dir.to_string());
    serde_json::to_string(&args).unwrap_or_else(|_| arguments.to_string())
}

fn inject_image_urls_from_refs(
    arguments: &str,
    room_id: &str,
    webdav_dir: &str,
    refs: &[AttachmentRef],
    image_pool: Option<&HashMap<String, Vec<CachedImage>>>,
) -> String {
    let mut args: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));
    args["room_id"] = serde_json::Value::String(room_id.to_string());
    args["webdav_dir"] = serde_json::Value::String(webdav_dir.to_string());
    let mut injected: Vec<serde_json::Value> = Vec::new();
    let prompt_lower = arguments.to_lowercase();
    // 1. User-attached images whose name appears in the prompt
    for r in refs {
        if prompt_lower.contains(&r.title.to_lowercase()) {
            injected.push(serde_json::Value::String(r.data_uri.clone()));
        }
    }
    // 2. Vision-fetched images from image_pool whose label appears in the prompt
    if let Some(pool) = image_pool {
        if let Some(images) = pool.get(room_id) {
            for ci in images {
                let label = format!("![{}]", ci.name);
                if prompt_lower.contains(&ci.name.to_lowercase()) || prompt_lower.contains(&label) {
                    injected.push(serde_json::Value::String(ci.data_uri.clone()));
                }
            }
        }
    }
    // 3. Merge with any agent-provided URLs (e.g. fal CDN from previous generation or share_url)
    if let Some(agent_urls) = args.get("image_urls").and_then(|v| v.as_array()) {
        for url in agent_urls {
            if let Some(s) = url.as_str() {
                if !injected.iter().any(|v| v.as_str() == Some(s)) {
                    injected.push(serde_json::Value::String(s.to_string()));
                }
            }
        }
    }
    if !injected.is_empty() {
        debug!("Image interception: injecting {} image URL(s) into image_gen args (sources: attachments={}, pool={}, agent-provided={})",
            injected.len(),
            refs.iter().filter(|r| prompt_lower.contains(&r.title.to_lowercase())).count(),
            if let Some(pool) = image_pool { pool.get(room_id).map(|v| v.len()).unwrap_or(0) } else { 0 },
            args.get("image_urls").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0));
        args["image_urls"] = serde_json::Value::Array(injected);
    }
    serde_json::to_string(&args).unwrap_or_else(|_| arguments.to_string())
}

fn replace_secret_refs(value: &str, secrets: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(pos) = remaining.find("secret:") {
        result.push_str(&remaining[..pos]);
        let after_prefix = &remaining[pos + 7..];
        let key_end = after_prefix
            .find(|c: char| !c.is_ascii_hexdigit() && c != '-')
            .unwrap_or(after_prefix.len());
        if key_end == 0 {
            result.push_str("secret:");
            remaining = after_prefix;
            continue;
        }
        let key = &after_prefix[..key_end];
        match secrets.get(key) {
            Some(secret_value) => {
                debug!("Resolved secret ref for UUID '{}'", key);
                result.push_str(secret_value);
            }
            None => {
                if key.len() >= 32 && key.contains('-') {
                    warn!("Secret UUID '{}' not found in secrets map", key);
                }
                result.push_str("secret:");
                result.push_str(key);
            }
        }
        remaining = &after_prefix[key_end..];
    }
    result.push_str(remaining);
    result
}

fn filter_secrets_by_host(entries: &[ResolvedSecret], args_json: &str) -> Option<HashMap<String, String>> {
    let args: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let url_str = args.get("url")?.as_str()?;

    let host = match url::Url::parse(url_str) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            let hostname = parsed.host_str()?;
            let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{scheme}://{hostname}{port}")
        }
        Err(e) => {
            warn!("Failed to parse URL in web_fetch args for secret host matching: {e}");
            return None;
        }
    };

    let map: HashMap<String, String> = entries
        .iter()
        .filter(|e| e.host == host)
        .map(|e| (e.uuid.clone(), e.value.clone()))
        .collect();

    if map.is_empty() {
        debug!("No secrets configured for host '{}'", host);
        None
    } else {
        debug!("Resolved {} secret(s) for host '{}': {:?}",
            map.len(), host,
            entries.iter().filter(|e| e.host == host).map(|e| &e.key).collect::<Vec<_>>());
        Some(map)
    }
}

fn resolve_secret_refs_deep(args_json: &str, secrets: &HashMap<String, String>) -> String {
    let mut args: serde_json::Value =
        serde_json::from_str(args_json).unwrap_or(serde_json::json!({}));

    resolve_json_value(&mut args, secrets);

    serde_json::to_string(&args).unwrap_or_else(|_| args_json.to_string())
}

fn resolve_json_value(value: &mut serde_json::Value, secrets: &HashMap<String, String>) {
    match value {
        serde_json::Value::String(s) => {
            if s.contains("secret:") {
                *s = replace_secret_refs(s, secrets);
            }
        }
        serde_json::Value::Object(obj) => {
            for v in obj.values_mut() {
                resolve_json_value(v, secrets);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                resolve_json_value(v, secrets);
            }
        }
        _ => {}
    }
}

async fn load_secrets_from_webdav(webdav: Option<&WebDavClient>, room_dir: &str) -> Option<Vec<ResolvedSecret>> {
    let client = webdav?;
    let path = format!("{room_dir}/secrets.toml");
    match client.read_file_to_string(&path).await {
        Ok(content) => match toml::from_str::<SecretsToml>(&content) {
            Ok(parsed) if !parsed.secrets.is_empty() => {
                let resolved: Vec<ResolvedSecret> = parsed.secrets.into_iter().map(|e| {
                    let uuid = generate_secret_uuid(&e.host, &e.key);
                    ResolvedSecret { uuid, host: e.host, key: e.key, value: e.value }
                }).collect();
                debug!("Loaded {} secret(s) from WebDAV (hosts: {:?})",
                    resolved.len(),
                    resolved.iter().map(|r| r.host.as_str()).collect::<std::collections::HashSet<_>>());
                Some(resolved)
            }
            Ok(_) => {
                debug!("secrets.toml loaded but contains no entries — skipping interception");
                None
            }
            Err(e) => {
                warn!("Failed to parse secrets.toml: {e}");
                None
            }
        },
        Err(WebDavError::NotFound(_)) => {
            debug!("No secrets.toml found on WebDAV");
            None
        }
        Err(e) => {
            warn!("Failed to load secrets.toml from WebDAV: {e}");
            None
        }
    }
}

fn compute_webdav_dir(room_name: &str, room_fname: &str, is_dm: bool) -> String {
    let name = if room_fname.is_empty() {
        room_name
    } else {
        room_fname
    };
    if is_dm {
        format!("d-{}", name)
    } else {
        format!("r-{}", name)
    }
}

fn parse_compression_output(output: &str, default_summary: &str) -> (String, Vec<String>) {
    // Split at "## Used Knowledge" marker
    let (summary_part, used_part) = if let Some(pos) = output.find("## Used Knowledge") {
        let s = &output[..pos];
        let u = &output[pos + "## Used Knowledge".len()..];
        (s.trim(), u.trim())
    } else {
        (output.trim(), "")
    };

    let summary = if summary_part.is_empty() {
        default_summary.to_string()
    } else {
        summary_part.to_string()
    };

    let used: Vec<String> = used_part
        .lines()
        .filter_map(|line| {
        let trimmed = line.trim().trim_start_matches(|c: char| c == '-' || c.is_whitespace());
            if trimmed.ends_with(".md") && !trimmed.is_empty() {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect();

    (summary, used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RockBotError;
    use crate::image_cache::GeneratedImage;
    use crate::validated::NonEmptyString;
    use crate::provider::AiProvider;
    use crate::tool::Tool;
    use crate::tools::WebFetchTool;
    use crate::types::{CompletionResult, ContentPart, FinishReason, MessageContent, ToolCall};
    use async_trait::async_trait;

    struct MockProvider {
        responses: std::sync::Mutex<Vec<Result<CompletionResult>>>,
        call_count: std::sync::atomic::AtomicUsize,
    }

    impl MockProvider {
        fn new(responses: Vec<CompletionResult>) -> Self {
            Self {
                responses: std::sync::Mutex::new(
                    responses.into_iter().map(Ok).collect(),
                ),
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn with_result_queue(responses: Vec<Result<CompletionResult>>) -> Self {
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
                responses.remove(0)
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

[model]
default_provider = "mock"
default_model = "chat"
max_iterations = 8

[[chat_providers]]
name = "mock"
api_key = "sk-mock"
base_url = "https://mock.ai/v1"

[chat_providers.models]
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

        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        let result = harness
            .process_message("room1", "general", "", false, "user", "Hi", &[], &[])
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

        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        let result = harness
            .process_message("dm-alice", "", "", true, "alice", "Hello bot", &[], &[])
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), "DM response");
    }

    #[tokio::test]
    async fn test_harness_provider_error() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));

        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        let result = harness
            .process_message("room1", "general", "", false, "user", "Hi", &[], &[])
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

[model]
default_provider = "mock"
default_model = "chat"
max_iterations = 2
max_summary_chars = 8000
summary_days = 7

[[chat_providers]]
name = "mock"
api_key = "sk-mock"
base_url = "https://mock.ai/v1"

[chat_providers.models]
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

        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let result = harness
            .process_message("room1", "general", "", false, "user", "search something", &[], &[])
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
        let harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        assert_eq!(harness.memory().room_count(), 0);
        assert_eq!(harness.tools().len(), 0);
    }

    #[test]
    fn test_system_prompt_contains_soul_template() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        let prompt = harness.build_system_prompt();
        assert!(
            prompt.contains("edit_soul tool"),
            "Prompt must reference the edit_soul tool"
        );
        assert!(
            prompt.contains("save_knowledge tool"),
            "Prompt must reference save_knowledge tool"
        );
        assert!(
            prompt.contains("forget_knowledge tool"),
            "Prompt must reference forget_knowledge tool"
        );
        assert!(
            prompt.contains("recall_knowledge tool"),
            "Prompt must reference recall_knowledge tool"
        );
        assert!(
            prompt.contains("only image_gen produces real images"),
            "Prompt must warn against fabricating fake image references"
        );
    }

    #[test]
    fn test_last_image_ids_empty_by_default() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        let ids = harness.take_last_image_ids();
        assert!(ids.is_empty());
    }

    #[test]
    fn test_take_image_ids_drains() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        // Store some ids
        harness.last_image_ids = vec!["call_a".into(), "call_b".into()];
        let ids = harness.take_last_image_ids();
        assert_eq!(ids, vec!["call_a", "call_b"]);
        // Should be drained
        let ids2 = harness.take_last_image_ids();
        assert!(ids2.is_empty());
    }

    #[test]
    fn test_take_image_from_cache() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let cache = Arc::new(ImageCache::new());
        cache.store("call_test", GeneratedImage {
            webdav_path: NonEmptyString::try_new("/r-test/images/img.png".to_string()).unwrap(),
            image_bytes: vec![1, 2, 3],
            mime_type: NonEmptyString::try_new("image/png".to_string()).unwrap(),
            share_url: Some("https://example.com/s/abc/download".into()),
        });
        let harness = AgentHarness::new(config, provider, None, cache.clone());
        let img = harness.take_image("call_test");
        assert!(img.is_some());
        let img = img.unwrap();
        assert_eq!(img.webdav_path.as_str(), "/r-test/images/img.png");
        assert_eq!(img.share_url.unwrap(), "https://example.com/s/abc/download");
        // Should be consumed
        assert!(harness.take_image("call_test").is_none());
    }

    #[test]
    fn test_image_cache_share_url_computed() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let cache = Arc::new(ImageCache::new());
        // Image with no share_url (simulating failed share creation)
        cache.store("call_no_share", GeneratedImage {
            webdav_path: NonEmptyString::try_new("/r-test/img.png".to_string()).unwrap(),
            image_bytes: vec![1, 2, 3],
            mime_type: NonEmptyString::try_new("image/png".to_string()).unwrap(),
            share_url: None,
        });
        let harness = AgentHarness::new(config, provider, None, cache);
        let img = harness.take_image("call_no_share").unwrap();
        assert!(img.share_url.is_none());
        // data_uri fallback should still work
        assert!(img.data_uri().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn test_resolve_model() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        let model = harness.resolve_model();
        assert_eq!(model, "mock-model");
    }

    #[tokio::test]
    async fn test_compress_room_if_needed_no_webdav() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..10 {
            room.history.append(ChatMessage::user(format!("msg {}", i)));
        }
        // Set byte_pressure_flag to trigger compression
        harness.memory_mut().set_byte_pressure("room1");

        let result = harness.compress_room_if_needed("room1").await;
        assert!(result.is_ok());
        // History should be pruned (5 messages: oldest half of 10)
        let remaining = harness.memory().get("room1").map(|r| r.history.messages.len());
        assert_eq!(remaining, Some(5));
    }

    #[test]
    fn test_check_and_archive_returns_seq() {
        let mut mgr = MemoryManager::new(2000, 60, 30_000_000);
        let room = mgr.get_or_create("room1", "general", "", false);
        for i in 0..10 {
            room.history.append(ChatMessage::user(format!(
                "Message number {} with some padding text",
                i
            )));
        }

        let result = mgr.check_and_archive("room1", false);
        assert!(result.is_some(), "Should archive with 10 messages (>4)");
        if let Some((rid, msgs)) = result {
            assert_eq!(rid, "room1");
            assert_eq!(msgs.len(), 5, "Should return oldest half (5 of 10)");
        }
    }

    #[tokio::test]
    async fn test_summarize_for_archive() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let msgs = vec![
            ChatMessage::user("Hello, I need help with something"),
            ChatMessage::assistant("Sure, what do you need?"),
        ];
        let (summary, used) = harness.compress_for_summary(&msgs, None, &[]).await;
        assert!(summary.contains("2 messages"));
        assert!(used.is_empty());
    }

    #[test]
    fn test_inject_room_context() {
        let args = r#"{"action":"read","path":"notes.txt"}"#;
        let result = inject_room_context(args, "general", "r-general");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["room_id"], "general");
        assert_eq!(parsed["webdav_dir"], "r-general");
        assert_eq!(parsed["action"], "read");
    }

    #[test]
    fn test_inject_room_context_for_image_gen() {
        let args = r#"{"prompt":"test","room_id":"x"}"#;
        let result = inject_room_context(args, "general", "r-general");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["room_id"], "general");
        assert_eq!(parsed["webdav_dir"], "r-general");
    }

    #[test]
    fn test_inject_image_urls_from_refs_matches_title() {
        let args = r#"{"prompt":"edit this apple.png for me"}"#;
        let refs = vec![
            AttachmentRef { title: "apple.png".into(), data_uri: "data:image/png;base64,abc".into() },
            AttachmentRef { title: "banana.jpg".into(), data_uri: "data:image/jpg;base64,xyz".into() },
        ];
        let result = inject_image_urls_from_refs(args, "general", "r-general", &refs, None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["room_id"], "general");
        assert_eq!(parsed["webdav_dir"], "r-general");
        let urls = parsed["image_urls"].as_array().unwrap();
        // Only apple.png is referenced in the prompt
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "data:image/png;base64,abc");
    }

    #[test]
    fn test_inject_image_urls_from_refs_no_match() {
        let args = r#"{"prompt":"edit this image"}"#;
        let refs = vec![
            AttachmentRef { title: "photo.png".into(), data_uri: "data:image/png;base64,abc".into() },
        ];
        let result = inject_image_urls_from_refs(args, "general", "r-general", &refs, None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        // No title match in prompt -> nothing injected
        assert!(parsed.get("image_urls").is_none());
    }

    #[test]
    fn test_inject_image_urls_from_refs_merges_agent_urls() {
        let args = r#"{"prompt":"edit photo.png","image_urls":["https://fal.media/prev.png"]}"#;
        let refs = vec![
            AttachmentRef { title: "photo.png".into(), data_uri: "data:image/png;base64,abc".into() },
        ];
        let result = inject_image_urls_from_refs(args, "general", "r-general", &refs, None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let urls = parsed["image_urls"].as_array().unwrap();
        // Both harness URI and agent-provided fal CDN URL should be present
        assert_eq!(urls.len(), 2);
        assert!(urls.iter().any(|v| v == "data:image/png;base64,abc"));
        assert!(urls.iter().any(|v| v == "https://fal.media/prev.png"));
    }

    #[test]
    fn test_compute_webdav_dir_channel() {
        assert_eq!(compute_webdav_dir("atomkb", "", false), "r-atomkb");
    }

    #[test]
    fn test_compute_webdav_dir_dm() {
        assert_eq!(compute_webdav_dir("saru", "", true), "d-saru");
    }

    #[test]
    fn test_compute_webdav_dir_channel_with_hyphens() {
        assert_eq!(
            compute_webdav_dir("my-team-room", "", false),
            "r-my-team-room"
        );
    }

    #[test]
    fn test_compute_webdav_dir_dm_with_dots() {
        assert_eq!(
            compute_webdav_dir("john.doe", "", true),
            "d-john.doe"
        );
    }

    #[test]
    fn test_compute_webdav_dir_unicode_name() {
        assert_eq!(compute_webdav_dir("日本語", "", false), "r-日本語");
        assert_eq!(compute_webdav_dir("中文", "", true), "d-中文");
    }

    #[test]
    fn test_compute_webdav_dir_empty_name() {
        assert_eq!(compute_webdav_dir("", "", false), "r-");
        assert_eq!(compute_webdav_dir("", "", true), "d-");
    }

    #[test]
    fn test_compute_webdav_dir_prefers_fname() {
        assert_eq!(
            compute_webdav_dir("sen1-lin2-sheng1-tai4", "森林生態", false),
            "r-森林生態"
        );
    }

    #[test]
    fn test_compute_webdav_dir_fallback_when_fname_empty() {
        assert_eq!(
            compute_webdav_dir("general", "", false),
            "r-general"
        );
    }

    #[tokio::test]
    async fn test_trim_context_below_limit() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let messages = vec![
            ChatMessage::system("You are a helpful assistant."),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
        ];

        let result = harness
            .trim_context("room1", messages.clone(), 1_000_000)
            .await;

        assert_eq!(result.len(), messages.len());
        for (a, b) in result.iter().zip(messages.iter()) {
            assert_eq!(a.role, b.role);
            assert_eq!(a.text_content(), b.text_content());
        }
    }

    #[tokio::test]
    async fn test_trim_context_above_limit() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        // Build messages that exceed the 1-byte limit (always triggers trim)
        let messages: Vec<ChatMessage> = (0..10)
            .map(|i| {
                ChatMessage::user(format!(
                    "Message number {} with enough padding text to make bytes count",
                    i
                ))
            })
            .collect();

        let input_len = messages.len();
        let result = harness
            .trim_context("room1", messages.clone(), 1)
            .await;

        assert!(
            result.len() < input_len,
            "Expected fewer messages after trim ({} -> {})",
            input_len,
            result.len()
        );
    }

    #[tokio::test]
    async fn test_trim_context_preserves_system_prompt() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let system_msg = ChatMessage::system("You are a helpful assistant.");
        let mut messages: Vec<ChatMessage> = vec![system_msg.clone()];
        for i in 0..12 {
            messages.push(ChatMessage::user(format!(
                "Message number {} with padding text for bytes",
                i
            )));
        }

        let result = harness
            .trim_context("room1", messages, 1)
            .await;

        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].text_content(), system_msg.text_content());
    }

    #[tokio::test]
    async fn test_trim_context_preserves_last_messages() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let last_user = ChatMessage::user("LAST user message to keep");
        let last_assistant = ChatMessage::assistant("LAST assistant message to keep");
        let mut messages: Vec<ChatMessage> = vec![ChatMessage::system("sys")];
        for i in 0..15 {
            messages.push(ChatMessage::user(format!("msg {}", i)));
        }
        messages.push(last_user.clone());
        messages.push(last_assistant.clone());

        let result = harness
            .trim_context("room1", messages, 1)
            .await;

        // Last messages should be preserved
        assert!(result.len() >= 3); // system + last 2
        let last_msg_content = result.last().unwrap().text_content();
        assert!(last_msg_content.is_some());
    }

    #[tokio::test]
    async fn test_process_message_mark_snapshot_dirty() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![CompletionResult {
            text: Some("Hello! How can I help?".into()),
            tool_calls: vec![],
            finish: FinishReason::Stop,
            reasoning_content: None,
            usage: None,
        }]));

        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        harness
            .process_message("room1", "general", "", false, "user", "Hi", &[], &[])
            .await
            .unwrap();

        let dirty = harness.memory().dirty_snapshots();
        assert!(
            dirty.contains(&"room1".to_string()),
            "room1 should be marked dirty after process_message, got: {:?}",
            dirty
        );
    }

    #[tokio::test]
    async fn test_process_message_appends_user_and_assistant() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![CompletionResult {
            text: Some("Hello! How can I help?".into()),
            tool_calls: vec![],
            finish: FinishReason::Stop,
            reasoning_content: None,
            usage: None,
        }]));

        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        harness
            .process_message("room1", "general", "", false, "user", "Hi", &[], &[])
            .await
            .unwrap();

        let room = harness.memory().get("room1").expect("room1 should exist");
        let roles: Vec<_> = room.history.messages.iter().map(|m| &m.role).collect();
        assert!(
            roles.contains(&&Role::User),
            "History should contain a User message, got: {:?}",
            roles
        );
        assert!(
            roles.contains(&&Role::Assistant),
            "History should contain an Assistant message, got: {:?}",
            roles
        );
    }

    #[tokio::test]
    async fn test_process_message_extracts_image_urls() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![CompletionResult {
            text: Some("Got it!".into()),
            tool_calls: vec![],
            finish: FinishReason::Stop,
            reasoning_content: None,
            usage: None,
        }]));
        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let msg_urls = vec![
            rocketchat::MessageUrl {
                url: "https://nc.example.com/s/img1/download".into(),
                meta: None,
                headers: Some(rocketchat::UrlHeaders {
                    content_length: Some("1000".into()),
                    content_type: Some("image/png".into()),
                }),
            },
            rocketchat::MessageUrl {
                url: "https://example.com/article".into(),
                meta: None,
                headers: Some(rocketchat::UrlHeaders {
                    content_length: Some("42000".into()),
                    content_type: Some("text/html".into()),
                }),
            },
        ];

        harness
            .process_message("room1", "general", "", false, "user", "edit this", &[], &msg_urls)
            .await
            .unwrap();

        // Only the image/png URL should be extracted, not text/html
        let urls = harness.current_image_urls();
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://nc.example.com/s/img1/download");
    }

    #[tokio::test]
    async fn test_process_message_no_image_urls_without_headers() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![CompletionResult {
            text: Some("OK".into()),
            tool_calls: vec![],
            finish: FinishReason::Stop,
            reasoning_content: None,
            usage: None,
        }]));
        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let msg_urls = vec![
            rocketchat::MessageUrl {
                url: "https://example.com/some-link".into(),
                meta: None,
                headers: None, // no headers = not identified as image
            },
        ];

        harness
            .process_message("room1", "general", "", false, "user", "Hi", &[], &msg_urls)
            .await
            .unwrap();

        assert!(harness.current_image_urls().is_empty());
    }

    #[tokio::test]
    async fn test_process_message_empty_urls_still_works() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![CompletionResult {
            text: Some("Hello!".into()),
            tool_calls: vec![],
            finish: FinishReason::Stop,
            reasoning_content: None,
            usage: None,
        }]));
        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));

        let result = harness
            .process_message("room1", "general", "", false, "user", "Hi", &[], &[])
            .await;

        assert!(result.is_ok());
        assert!(harness.current_image_urls().is_empty());
    }

    // ----- ContextLengthExceeded retry tests (agent-harness.md §2i2) -----

    #[tokio::test]
    async fn test_context_length_exceeded_retry_compresses_and_succeeds() {
        let config = make_test_config();

        // Provider: first call returns ContextLengthExceeded, second call succeeds
        let provider = Box::new(MockProvider::with_result_queue(vec![
            Err(RockBotError::ContextLengthExceeded(
                "Request parameters validation failed: max_tokens is too large for the given context length".into(),
            )),
            Ok(CompletionResult {
                text: Some("Compressed and retried!".into()),
                tool_calls: vec![],
                finish: FinishReason::Stop,
                reasoning_content: None,
                usage: None,
            }),
        ]));

        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        let result = harness
            .process_message("room1", "general", "", false, "user", "Long context message", &[], &[])
            .await;

        assert!(result.is_ok());
        let reply = result.unwrap();
        assert!(reply.is_some());
        assert!(reply.unwrap().contains("Compressed and retried"));
    }

    #[tokio::test]
    async fn test_context_length_exceeded_double_failure_falls_back() {
        let config = make_test_config();

        // Provider: both calls return ContextLengthExceeded (no recovery)
        let provider = Box::new(MockProvider::with_result_queue(vec![
            Err(RockBotError::ContextLengthExceeded("context too long".into())),
            Err(RockBotError::ContextLengthExceeded("still too long".into())),
        ]));

        let mut harness = AgentHarness::new(config, provider, None, Arc::new(ImageCache::new()));
        let result = harness
            .process_message("room1", "general", "", false, "user", "Hi", &[], &[])
            .await;

        assert!(result.is_ok());
        let reply = result.unwrap();
        assert!(reply.is_some());
        assert!(
            reply.unwrap().contains("error"),
            "Double CE error should produce error fallback reply"
        );
    }

    #[test]
    fn test_replace_secret_refs_simple() {
        let mut secrets = HashMap::new();
        secrets.insert("a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".to_string(), "abc123".to_string());
        assert_eq!(
            replace_secret_refs("token secret:a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789", &secrets),
            "token abc123"
        );
    }

    #[test]
    fn test_replace_secret_refs_bare_reference() {
        let mut secrets = HashMap::new();
        secrets.insert("e5f6a7b8-c9d0-41e1-f2a3-b4c5d6e7f890".to_string(), "sk-xyz".to_string());
        assert_eq!(
            replace_secret_refs("secret:e5f6a7b8-c9d0-41e1-f2a3-b4c5d6e7f890", &secrets),
            "sk-xyz"
        );
    }

    #[test]
    fn test_replace_secret_refs_multiple_in_one_value() {
        let mut secrets = HashMap::new();
        secrets.insert("a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".to_string(), "aaa".to_string());
        secrets.insert("f8a9b0c1-d2e3-45f4-a5b6-c7d8e9f0a1b2".to_string(), "bbb".to_string());
        let input = "secret:a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789:secret:f8a9b0c1-d2e3-45f4-a5b6-c7d8e9f0a1b2";
        assert_eq!(replace_secret_refs(input, &secrets), "aaa:bbb");
    }

    #[test]
    fn test_replace_secret_refs_missing_uuid_preserves_original() {
        let mut secrets = HashMap::new();
        secrets.insert("a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".to_string(), "value".to_string());
        // non-UUID keys are not resolved
        assert_eq!(
            replace_secret_refs("secret:gitea_token", &secrets),
            "secret:gitea_token"
        );
        // unknown UUID preserved
        assert_eq!(
            replace_secret_refs("secret:00000000-0000-0000-0000-000000000000", &secrets),
            "secret:00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn test_replace_secret_refs_no_refs() {
        let secrets = HashMap::new();
        assert_eq!(
            replace_secret_refs("application/json", &secrets),
            "application/json"
        );
    }

    #[test]
    fn test_replace_secret_refs_preserves_surrounding_text() {
        let mut secrets = HashMap::new();
        secrets.insert("a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".to_string(), "real".to_string());
        assert_eq!(
            replace_secret_refs("Bearer secret:a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789 extra", &secrets),
            "Bearer real extra"
        );
    }

    #[test]
    fn test_replace_secret_refs_non_uuid_key_passthrough() {
        let mut secrets = HashMap::new();
        secrets.insert("gitea_token".to_string(), "should_not_resolve".to_string());
        assert_eq!(
            replace_secret_refs("token secret:gitea_token", &secrets),
            "token secret:gitea_token"
        );
    }

    #[test]
    fn test_replace_secret_refs_adjacent_non_key_char() {
        let secrets = HashMap::new();
        assert_eq!(
            replace_secret_refs("secret: followed by space", &secrets),
            "secret: followed by space"
        );
    }

    #[test]
    fn test_replace_secret_refs_trailing_prefix() {
        let secrets = HashMap::new();
        assert_eq!(
            replace_secret_refs("ends with secret:", &secrets),
            "ends with secret:"
        );
    }

    #[test]
    fn test_resolve_secret_refs_deep_headers() {
        let mut secrets = HashMap::new();
        secrets.insert("a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".to_string(), "abc123".to_string());
        let args = r#"{"url":"https://example.com","headers":{"Authorization":"token secret:a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789"}}"#;
        let result = resolve_secret_refs_deep(args, &secrets);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["headers"]["Authorization"].as_str().unwrap(), "token abc123");
    }

    #[test]
    fn test_resolve_secret_refs_deep_url() {
        let mut secrets = HashMap::new();
        secrets.insert("e5f6a7b8-c9d0-41e1-f2a3-b4c5d6e7f890".to_string(), "sk-xyz".to_string());
        let args = r#"{"url":"https://api.example.com/v1?token=secret:e5f6a7b8-c9d0-41e1-f2a3-b4c5d6e7f890"}"#;
        let result = resolve_secret_refs_deep(args, &secrets);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["url"].as_str().unwrap(), "https://api.example.com/v1?token=sk-xyz");
    }

    #[test]
    fn test_resolve_secret_refs_deep_body() {
        let mut secrets = HashMap::new();
        secrets.insert("a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".to_string(), "abc123".to_string());
        let args = r#"{"url":"https://example.com","body":"auth=secret:a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789"}"#;
        let result = resolve_secret_refs_deep(args, &secrets);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["body"].as_str().unwrap(), "auth=abc123");
    }

    #[test]
    fn test_resolve_secret_refs_deep_body_json() {
        let mut secrets = HashMap::new();
        secrets.insert("f8a9b0c1-d2e3-45f4-a5b6-c7d8e9f0a1b2".to_string(), "ghp-xyz".to_string());
        let args = r#"{"url":"https://example.com","body_json":{"pat":"secret:f8a9b0c1-d2e3-45f4-a5b6-c7d8e9f0a1b2"}}"#;
        let result = resolve_secret_refs_deep(args, &secrets);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["body_json"]["pat"].as_str().unwrap(), "ghp-xyz");
    }

    #[test]
    fn test_resolve_secret_refs_deep_multiple() {
        let mut secrets = HashMap::new();
        secrets.insert("aaa11111-bbbb-4ccc-8ddd-eeeeeeeeeee1".to_string(), "real_tok".to_string());
        secrets.insert("aaa11111-bbbb-4ccc-8ddd-eeeeeeeeeee2".to_string(), "val2".to_string());
        let args = r#"{"url":"https://api.example.com?key=secret:aaa11111-bbbb-4ccc-8ddd-eeeeeeeeeee2","headers":{"Authorization":"Bearer secret:aaa11111-bbbb-4ccc-8ddd-eeeeeeeeeee1"},"body":"param=secret:aaa11111-bbbb-4ccc-8ddd-eeeeeeeeeee2"}"#;
        let result = resolve_secret_refs_deep(args, &secrets);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["headers"]["Authorization"].as_str().unwrap(), "Bearer real_tok");
        assert_eq!(parsed["url"].as_str().unwrap(), "https://api.example.com?key=val2");
        assert_eq!(parsed["body"].as_str().unwrap(), "param=val2");
    }

    #[test]
    fn test_resolve_secret_refs_deep_no_secret_refs() {
        let mut secrets = HashMap::new();
        secrets.insert("a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".to_string(), "value".to_string());
        let args = r#"{"headers":{"Content-Type":"application/json"},"url":"https://example.com"}"#;
        let result = resolve_secret_refs_deep(args, &secrets);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["headers"]["Content-Type"].as_str().unwrap(), "application/json");
    }

    #[test]
    fn test_resolve_secret_refs_deep_empty_secrets() {
        let secrets = HashMap::new();
        let args = r#"{"headers":{"Authorization":"secret:a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789"},"url":"https://x.com?k=secret:e5f6a7b8-c9d0-41e1-f2a3-b4c5d6e7f890"}"#;
        let result = resolve_secret_refs_deep(args, &secrets);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["headers"]["Authorization"].as_str().unwrap(), "secret:a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789");
        assert_eq!(parsed["url"].as_str().unwrap(), "https://x.com?k=secret:e5f6a7b8-c9d0-41e1-f2a3-b4c5d6e7f890");
    }

    #[test]
    fn test_resolve_secret_refs_deep_ignores_non_uuid_keys() {
        let mut secrets = HashMap::new();
        secrets.insert("a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".to_string(), "resolved".to_string());
        let args = r#"{"url":"https://example.com","headers":{"Authorization":"Bearer secret:gitea_token"}}"#;
        let result = resolve_secret_refs_deep(args, &secrets);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["headers"]["Authorization"].as_str().unwrap(), "Bearer secret:gitea_token");
    }

    #[tokio::test]
    async fn test_load_secrets_from_webdav_with_mock_server() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/test-room/secrets.toml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[[secrets]]
host = "https://example.com"
key = "gitea_token"
value = "abc123"

[[secrets]]
host = "https://example.com"
key = "github_api_key"
value = "sk-xyz"
"#,
            ))
            .mount(&mock_server)
            .await;

        let client = WebDavClient::new(mock_server.uri(), "test", "pass")
            .expect("valid webdav client");
        let secrets = load_secrets_from_webdav(Some(&client), "test-room").await;

        assert!(secrets.is_some(), "secrets should be loaded");
        let entries = secrets.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].host, "https://example.com");
        assert_eq!(entries[0].key, "gitea_token");
        assert_eq!(entries[0].value, "abc123");
        assert_eq!(entries[0].uuid.len(), 36, "UUID should be 36 chars");
        assert_eq!(entries[1].key, "github_api_key");
        assert_eq!(entries[1].value, "sk-xyz");
        assert_ne!(entries[0].uuid, entries[1].uuid, "different keys must get different UUIDs");
    }

    #[tokio::test]
    async fn test_load_secrets_webdav_not_found_returns_none() {
        use wiremock::MockServer;

        let mock_server = MockServer::start().await;
        let client = WebDavClient::new(mock_server.uri(), "test", "pass")
            .expect("valid webdav client");
        let secrets = load_secrets_from_webdav(Some(&client), "test-room").await;
        assert!(secrets.is_none(), "should return None when file not found");
    }

    #[tokio::test]
    async fn test_secret_interception_end_to_end() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/test-room/secrets.toml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                r#"[[secrets]]
host = "{}"
key = "gitea_token"
value = "real_gitea_token_123"
"#,
                mock_server.uri()
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/test"))
            .and(header("Authorization", "token real_gitea_token_123"))
            .respond_with(ResponseTemplate::new(200).set_body_string("authorized"))
            .mount(&mock_server)
            .await;

        let client = WebDavClient::new(mock_server.uri(), "test", "pass")
            .expect("valid webdav client");
        let entries = load_secrets_from_webdav(Some(&client), "test-room").await.unwrap();
        let secret_uuid = &entries[0].uuid;

        let args = serde_json::json!({
            "url": format!("{}/api/test", mock_server.uri()),
            "headers": {
                "Authorization": format!("token secret:{secret_uuid}")
            },
            "format": "raw"
        })
        .to_string();

        let host_map = filter_secrets_by_host(&entries, &args).unwrap();
        let resolved_args = resolve_secret_refs_deep(&args, &host_map);
        let parsed: serde_json::Value = serde_json::from_str(&resolved_args).unwrap();
        assert_eq!(
            parsed["headers"]["Authorization"].as_str().unwrap(),
            "token real_gitea_token_123"
        );

        let tool = WebFetchTool::with_webdav(client);
        let result = tool.execute(&resolved_args).await.unwrap();
        assert!(result.contains("authorized"), "web_fetch should succeed with resolved secret, got: {result}");
    }

    #[test]
    fn test_filter_secrets_by_host_matches() {
        let uuid1 = generate_secret_uuid("https://example.com", "tok");
        let uuid2 = generate_secret_uuid("https://other.com", "other_tok");
        let entries = vec![
            ResolvedSecret { uuid: uuid1.clone(), host: "https://example.com".into(), key: "tok".into(), value: "abc".into() },
            ResolvedSecret { uuid: uuid2.clone(), host: "https://other.com".into(), key: "other_tok".into(), value: "xyz".into() },
        ];
        let args = r#"{"url":"https://example.com/api"}"#;
        let map = filter_secrets_by_host(&entries, args).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&uuid1).unwrap(), "abc");
        assert!(!map.contains_key(&uuid2));
    }

    #[test]
    fn test_filter_secrets_by_host_no_match() {
        let uuid = generate_secret_uuid("https://example.com", "tok");
        let entries = vec![ResolvedSecret { uuid, host: "https://example.com".into(), key: "tok".into(), value: "abc".into() }];
        let args = r#"{"url":"https://other.com/api"}"#;
        assert!(filter_secrets_by_host(&entries, args).is_none());
    }

    #[test]
    fn test_filter_secrets_by_host_multiple_same_host() {
        let uuid1 = generate_secret_uuid("https://example.com", "tok1");
        let uuid2 = generate_secret_uuid("https://example.com", "tok2");
        let entries = vec![
            ResolvedSecret { uuid: uuid1.clone(), host: "https://example.com".into(), key: "tok1".into(), value: "abc".into() },
            ResolvedSecret { uuid: uuid2.clone(), host: "https://example.com".into(), key: "tok2".into(), value: "def".into() },
        ];
        let args = r#"{"url":"https://example.com/api"}"#;
        let map = filter_secrets_by_host(&entries, args).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&uuid1).unwrap(), "abc");
        assert_eq!(map.get(&uuid2).unwrap(), "def");
    }

    #[test]
    fn test_generate_secret_uuid_deterministic() {
        let a = generate_secret_uuid("https://gitea.example.com", "gitea_token");
        let b = generate_secret_uuid("https://gitea.example.com", "gitea_token");
        assert_eq!(a, b, "UUIDv5 must be deterministic");
        let c = generate_secret_uuid("https://gitea.example.com", "webhook_secret");
        assert_ne!(a, c, "different keys must produce different UUIDs");
        let d = generate_secret_uuid("https://api.github.com", "gitea_token");
        assert_ne!(a, d, "different hosts must produce different UUIDs");
    }

    #[test]
    fn test_build_secret_uuids_prompt() {
        let entries = vec![
            ResolvedSecret { uuid: "a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789".into(), host: "https://gitea.example.com".into(), key: "gitea_token".into(), value: "abc".into() },
            ResolvedSecret { uuid: "e5f6a7b8-c9d0-41e1-f2a3-b4c5d6e7f890".into(), host: "https://api.github.com".into(), key: "github_pat".into(), value: "xyz".into() },
        ];
        let prompt = build_secret_uuids_prompt(&entries);
        assert!(prompt.contains("secret:a1b2c3d4-e5f6-4789-a0b1-c2d3e4f56789 (gitea_token)"));
        assert!(prompt.contains("secret:e5f6a7b8-c9d0-41e1-f2a3-b4c5d6e7f890 (github_pat)"));
        assert!(!prompt.contains("gitea.example.com"), "host name must not appear in prompt");
        assert!(!prompt.contains("api.github.com"), "host name must not appear in prompt");
    }
}
