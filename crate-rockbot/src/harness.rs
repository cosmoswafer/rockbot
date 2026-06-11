use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use tracing::{debug, error, info, warn};
use webdav::{WebDavClient, WebDavPath};
use rocketchat::RestApiClient;

use crate::AppConfig;
use crate::error::Result;
use crate::knowledge::KnowledgeManager;
use crate::memory::{DailySummary, MemoryManager, SoulMemory};
use crate::provider::AiProvider;
use crate::tool::ToolRegistry;
use crate::types::{ChatMessage, ChatRequest, Role};
use crate::utils::{now_iso_string, today_iso_date};

const DEFAULT_SYSTEM_PROMPT: &str = "\
You are {name}, a helpful AI assistant running on a RocketChat server. \
You respond to DMs and @mentions concisely and helpfully. \
When you need the current date or time, use the datetime tool. \
When you need information from the web, use the web_search tool. \
When you need to fetch a URL, use web_fetch. \
When you need to fetch an image from a WebDAV path or public URL, use the vision tool. \
When you need to read, write, list, or manage files on remote storage, use the webdav tool. \
When you need to manage calendar events or todo tasks, use the calendar tool. \
When you need to generate an image, use the image_gen tool. \
When a user sends an image and asks to edit, modify, transform, or use it \
as a basis for image generation, use the image_gen tool — user-attached images \
appear as markdown ![image_name](image_name) in the conversation. Reference the \
image by its image_name in your prompt (e.g. \"edit image1.png to add a hat\"). \
The harness will automatically resolve image_name references to the actual images. \
If the user asks to edit a previously generated image (no new attachment), \
you MUST include the fal.ai CDN URL from the previous result in the \
image_urls parameter yourself. \
The image_gen tool returns both a WebDAV path and an original fal.ai CDN URL — \
always share the fal.ai CDN URL with the user in markdown image format \
as `![{description}]({fal_url})` so they can view the image inline. \
When a user says !soul or asks to save or update preferences, identity, or facts, use the edit_soul tool. \
When a user asks you to remember something, shares notes, or says !remember, !note, !save or shares important \
information worth persisting, use the save_knowledge tool. \
When a user says !forget or asks to remove something you learned, \
use the forget_knowledge tool. \
When you need to recall previously saved knowledge, use the recall_knowledge tool. \
Your display name is the first non-heading line of your soul file. \
When setting your name via edit_soul, create an Identity section with \
your name on its own line (e.g. \"## Identity\\n零夢\"). \
Use a short name under 32 characters. \
Answer in the same language as the user. \
Keep responses clear and to the point.\
";

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
        let max_summary_chars = config.rocketchat.model.max_summary_chars;
        let summary_days = config.rocketchat.model.summary_days;
        let max_soul_chars = config.rocketchat.model.max_soul_chars;
        let persist_interval = config.rocketchat.model.persist_interval_secs;
        let max_context_bytes = config.rocketchat.model.max_context_bytes;
        let max_attachment_bytes = config.rocketchat.model.max_attachment_bytes;
        let config = Arc::new(config);
        Self {
            config,
            provider,
            memory: MemoryManager::new(max_chars, max_history, max_summary_chars, summary_days, max_soul_chars, persist_interval, max_context_bytes),
            tools: ToolRegistry::new(),
            webdav,
            rest_client: None,
            max_iterations,
            max_attachment_bytes,
            image_pool: HashMap::new(),
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

    pub fn set_rest_client(&mut self, client: RestApiClient) {
        self.rest_client = Some(client);
    }

    pub fn has_rest_client(&self) -> bool {
        self.rest_client.is_some()
    }

    pub async fn resolve_room_fname(&mut self, room_id: &str) -> Option<String> {
        if let Some(ref mut client) = self.rest_client {
            client.resolve_room_fname(room_id).await
        } else {
            None
        }
    }

    pub async fn process_message(
        &mut self,
        room_id: &str,
        room_name: &str,
        room_fname: &str,
        is_dm: bool,
        sender_name: &str,
        text: &str,
        attachments: &[rocketchat::AttachmentInfo],
    ) -> Result<Option<String>> {
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
                format!("Describe this image in detail.\nAttached: {}", image_labels)
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

        let system_prompt = self.build_system_prompt();
        let tool_defs = self.tools.definitions();
        let have_tools = !tool_defs.is_empty();

        let model = self.resolve_model();

        let wd = compute_webdav_dir(room_name, room_fname, is_dm);
        if let Err(e) = self.refresh_knowledge_context(room_id, &wd).await {
            warn!("Failed to refresh knowledge context: {}", e);
        }

        let mut messages = self
            .memory
            .build_context(room_id, &system_prompt, None, None);
        self.inject_vision_images(room_id, &mut messages);
        debug!(
            "Built context for room {}: {} messages (model={}, have_tools={})",
            room_id,
            messages.len(),
            model,
            have_tools,
        );

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

                        let mut altered_soul = false;
                        let mut altered_knowledge = false;

                        for tool_call in &result.tool_calls {
                            debug!(
                                "Executing tool {} (call_id: {})",
                                tool_call.function.name, tool_call.id
                            );

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
                                inject_image_urls_from_refs(
                                    &tool_call.function.arguments,
                                    room_id,
                                    &wd,
                                    &attachment_refs,
                                )
                            } else if tool_call.function.name == "webdav"
                                || tool_call.function.name == "edit_soul"
                                || tool_call.function.name == "save_knowledge"
                                || tool_call.function.name == "forget_knowledge"
                                || tool_call.function.name == "recall_knowledge"
                                || tool_call.function.name == "calendar"
                            {
                                let wd = compute_webdav_dir(room_name, room_fname, is_dm);
                                inject_room_context(&tool_call.function.arguments, room_id, &wd)
                            } else {
                                tool_call.function.arguments.clone()
                            };

                            let tool_result = self
                                .tools
                                .execute_by_name(&tool_call.id, &tool_call.function.name, &arguments)
                                .await?;

                            if tool_call.function.name == "vision" && !tool_result.is_error {
                                self.cache_vision_images(room_id, &tool_result.content);
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
                        }

                        messages = self
                            .memory
                            .build_context(room_id, &system_prompt, None, None);
                        self.inject_vision_images(room_id, &mut messages);
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
        let name = &self.config.rocketchat.server.username;
        DEFAULT_SYSTEM_PROMPT.replace("{name}", name)
    }

    fn resolve_model(&self) -> String {
        self.config
            .resolve_chat_model(
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
            let url = format!(
                "https://{}{}",
                self.config.rocketchat.server.url.trim_end_matches('/'),
                title_link
            );
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

    fn cache_vision_images(&mut self, room_id: &str, result: &str) {
        // Parse markdown image tags from vision tool result:
        // ![name](data:mime/type;base64,...)
        let mut remaining = result;
        while let Some(start) = remaining.find("![") {
            let after_bang = &remaining[start + 2..];
            if let Some(alt_end) = after_bang.find("](") {
                let name = &after_bang[..alt_end];
                let after_alt = &after_bang[alt_end + 2..];
                if let Some(paren_end) = after_alt.find(')') {
                    let url = &after_alt[..paren_end];
                    if url.starts_with("data:") {
                        debug!(
                            "Vision tool: caching image '{}' for room {}",
                            name, room_id
                        );
                        self.image_pool
                            .entry(room_id.to_string())
                            .or_default()
                            .push(CachedImage {
                                data_uri: url.to_string(),
                                name: name.to_string(),
                            });
                    }
                    remaining = &after_alt[paren_end + 1..];
                    continue;
                }
            }
            break;
        }
    }

    fn inject_vision_images(&mut self, room_id: &str, messages: &mut Vec<ChatMessage>) {
        if let Some(images) = self.image_pool.remove(room_id) {
            let count = images.len();
            if count == 0 {
                return;
            }
            let labels: Vec<String> = images
                .iter()
                .enumerate()
                .map(|(i, ci)| {
                    let idx = i + 1;
                    let ext = ci.name.rfind('.').map(|p| &ci.name[p..]).unwrap_or(".png");
                    format!("photo{}{}", idx, ext)
                })
                .collect();
            let data_uris: Vec<String> = images.into_iter().map(|ci| ci.data_uri).collect();
            let prompt = format!(
                "The requested image{} visible below:\nAttached: {}",
                if count > 1 { "s are" } else { " is" },
                labels
                    .iter()
                    .map(|l| format!("![{}]({})", l, l))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            debug!(
                "Injecting {} vision image(s) for room {} into LLM context",
                count, room_id
            );
            messages.push(ChatMessage::user_with_images(prompt, data_uris));
        }
    }

    fn provider_http_client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }

    pub async fn archive_room_if_needed(&mut self, room_id: &str) -> Result<()> {
        let needs_archive = self.memory.check_and_archive(room_id);
        if let Some((rid, msgs)) = needs_archive {
            if let Some(ref webdav_client) = self.webdav {
                let count = msgs.len();
                let summary = self.summarize_for_archive(&msgs).await;

                let wd = {
                    let room = self.memory.get(&rid);
                    let (rn, rf, dm) = room
                        .map(|r| (r.room_name.as_str(), r.room_fname.as_str(), r.is_dm))
                        .unwrap_or((&rid, "", false));
                    compute_webdav_dir(rn, rf, dm)
                };

                // Layer 2: write daily .md summary
                let char_count = msgs
                    .iter()
                    .filter_map(|m| m.text_content())
                    .map(|t| t.chars().count())
                    .sum();
                let summary_ok = self.upsert_daily_summary(webdav_client, &wd, &summary, count, char_count).await.is_ok();
                if !summary_ok {
                    warn!("Failed to write daily summary, skipping prune");
                }

                if summary_ok {
                    // Mark snapshot dirty after Layer 2 write
                    self.memory.mark_snapshot_dirty(&rid);

                    // Age out old summaries
                    let summary_days = self.memory.summary_days;
                    if let Err(e) = self.delete_old_summaries(webdav_client, &wd, summary_days).await {
                        warn!("Failed to clean up old summaries: {}", e);
                    }

                    self.memory.prune_archived(&rid, count);
                }
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

    async fn upsert_daily_summary(
        &self,
        webdav: &WebDavClient,
        webdav_dir: &str,
        new_summary: &str,
        msg_count: usize,
        char_count: usize,
    ) -> Result<()> {
        let today = today_iso_date();
        let path = format!("{}memory/summaries/{}.md", WebDavPath::new("").room_dir(webdav_dir), today);

        let header = format!(
            "## {} ({} messages, {} chars)\n{}\n\n",
            today, msg_count, char_count, new_summary
        );

        let folder = format!("{}memory/summaries/", WebDavPath::new("").room_dir(webdav_dir));
        if let Err(e) = webdav.ensure_directory_all(&folder).await {
            warn!("Failed to ensure summaries directory {}: {}", folder, e);
        }

        let content = match webdav.read_file_to_string(&path).await {
            Ok(existing) => format!("{}{}", existing, header),
            Err(_) => {
                let title = format!("# Daily Summaries — {}\n\n", webdav_dir);
                format!("{}{}", title, header)
            }
        };

        webdav
            .write_file_with_fallback(&path, content.as_bytes().to_vec())
            .await
            .map_err(|e| crate::error::RockBotError::Provider(format!("Daily summary write failed: {e}")))?;

        debug!(
            "Upserted daily summary at {} ({} messages, {} chars)",
            path, msg_count, char_count
        );
        Ok(())
    }

    async fn delete_old_summaries(
        &self,
        webdav: &WebDavClient,
        webdav_dir: &str,
        max_days: u32,
    ) -> Result<()> {
        let folder = format!("{}memory/summaries/", WebDavPath::new("").room_dir(webdav_dir));
        let entries = match webdav.list_directory(&folder).await {
            Ok(e) => e,
            Err(_) => {
                debug!("No summaries directory yet at {}", folder);
                return Ok(());
            }
        };

        let today = {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (now.as_secs() / 86400) as i64
        };

        for entry in &entries {
            if entry.is_dir || !entry.name.ends_with(".md") {
                continue;
            }
            let date_str = entry.name.trim_end_matches(".md");
            if let Some(days) = crate::memory::date_to_days(date_str) {
                if today - days > max_days as i64 {
                    let path = format!("{}{}", folder, entry.name);
                    if let Err(e) = webdav.delete(&path).await {
                        warn!("Failed to delete old summary {}: {}", path, e);
                    } else {
                        debug!("Deleted old daily summary: {}", path);
                    }
                }
            }
        }
        Ok(())
    }

    async fn summarize_for_archive(&self, messages: &[ChatMessage]) -> String {
        if messages.is_empty() {
            return String::new();
        }

        let user_msgs: Vec<String> = messages
            .iter()
            .filter(|m| m.role == Role::User || m.role == Role::Assistant)
            .filter_map(|m| m.text_content())
            .map(|t| t.chars().take(300).collect::<String>())
            .take(20)
            .collect();

        if user_msgs.is_empty() {
            return format!("{} messages archived", messages.len());
        }

        let prompt = format!(
            "Summarize this conversation excerpt in 1-3 concise sentences. Focus on key topics, \
             decisions, and factual information shared:\n\n{}",
            user_msgs.join("\n")
        );

        let request = ChatRequest {
            model: self.resolve_model(),
            messages: vec![ChatMessage::user(&prompt)],
            tools: None,
            stream: false,
            temperature: Some(0.3),
            max_tokens: Some(256),
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        match self.provider.complete(request).await {
            Ok(result) => result.text.unwrap_or_else(|| {
                format!("{} messages archived", messages.len())
            }),
            Err(e) => {
                warn!("AI summarization failed, using fallback: {}", e);
                let preview_parts: Vec<String> = messages
                    .iter()
                    .take(5)
                    .filter_map(|m| m.text_content())
                    .map(|t| if t.len() > 80 { format!("{}...", &t[..80]) } else { t.to_string() })
                    .collect();
                if preview_parts.is_empty() {
                    format!("{} messages archived", messages.len())
                } else {
                    format!("{} messages: {}", messages.len(), preview_parts.join(" | "))
                }
            }
        }
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
        let mut got_summaries = false;

        if let Ok(content) = webdav_client.read_file_to_string(&snap_path).await {
            if let Ok(snapshot) = serde_json::from_str::<crate::memory::PersistSnapshot>(&content) {
                // Schema version check: reject unknown schemas
                if snapshot.schema == "rockbot-snapshot/1" {
                    self.memory.restore_snapshot(&snapshot);
                    got_soul = snapshot.soul.is_some();
                    got_summaries = !snapshot.daily_summaries.is_empty();
                    debug!(
                        "Restored snapshot for room {} (soul={}, summaries={})",
                        room_name, got_soul, got_summaries
                    );
                } else {
                    warn!(
                        "Unknown snapshot schema '{}' for room {}, using individual files",
                        snapshot.schema, room_name
                    );
                }
            }
        }

        // Fallback: load individual files for any missing layers
        if !got_summaries {
            match self.load_daily_summaries(webdav_client, &wd).await {
                Ok(summaries) if !summaries.is_empty() => {
                    debug!(
                        "Loaded {} daily summaries for room {}",
                        summaries.len(),
                        room_name
                    );
                    self.memory.set_daily_summaries(room_id, summaries);
                }
                Ok(_) => {
                    debug!("No daily summaries found for room {}", room_name);
                }
                Err(e) => {
                    warn!(
                        "Failed to load daily summaries for room {}: {}",
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

    async fn load_daily_summaries(
        &self,
        webdav: &WebDavClient,
        webdav_dir: &str,
    ) -> Result<Vec<DailySummary>> {
        let folder = format!("{}memory/summaries/", WebDavPath::new("").room_dir(webdav_dir));
        let entries = match webdav.list_directory(&folder).await {
            Ok(e) => e,
            Err(_) => {
                debug!("No summaries directory yet at {} (loading daily summaries)", folder);
                return Ok(Vec::new());
            }
        };

        let mut summaries = Vec::new();
        for entry in entries {
            if entry.is_dir || !entry.name.ends_with(".md") {
                continue;
            }
            let date_str = entry.name.trim_end_matches(".md");
            let path = format!("{}{}", folder, entry.name);
            match webdav.read_file_to_string(&path).await {
                Ok(content) => {
                    let summary_text = extract_latest_summary(&content);
                    let (msg_count, char_count) = parse_summary_header(&content);
                    if !summary_text.is_empty() {
                        summaries.push(DailySummary {
                            date: date_str.to_string(),
                            summary: summary_text,
                            msg_count,
                            char_count,
                        });
                    }
                }
                Err(e) => {
                    warn!("Failed to read daily summary {}: {}", path, e);
                }
            }
        }

        summaries.sort_by(|a, b| a.date.cmp(&b.date));
        Ok(summaries)
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
                    room_id: webdav_dir.to_string(),
                    content: String::new(),
                    updated_at: String::new(),
                });
            }
        };

        let updated_at = now_iso_string();

        Ok(SoulMemory {
            room_id: webdav_dir.to_string(),
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
                        "[Knowledge: {}/{}]\nUse when: {}\n{}",
                        entry.category, entry.title, entry.when_useful, body
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
        let webdav = self.webdav.clone();
        if let Some(ref webdav) = webdav {
            let text = self
                .load_knowledge_for_room(webdav, room_id, webdav_dir)
                .await?;
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
                && snapshot.daily_summaries.is_empty()
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
) -> String {
    let mut args: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));
    args["room_id"] = serde_json::Value::String(room_id.to_string());
    args["webdav_dir"] = serde_json::Value::String(webdav_dir.to_string());
    // If the agent provided image_urls (e.g. fal CDN URLs from a previous
    // generation), keep them. Always also inject harness-tagged data URIs
    // whose alt-text appears in the prompt.
    let mut injected: Vec<serde_json::Value> = Vec::new();
    let prompt_lower = arguments.to_lowercase();
    for r in refs {
        if prompt_lower.contains(&r.title.to_lowercase()) {
            injected.push(serde_json::Value::String(r.data_uri.clone()));
        }
    }
    // Merge with any agent-provided URLs (e.g. fal CDN from previous generation)
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
        args["image_urls"] = serde_json::Value::Array(injected);
    }
    serde_json::to_string(&args).unwrap_or_else(|_| arguments.to_string())
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

fn extract_latest_summary(daily_md: &str) -> String {
    // Extracts the most recent summary section (last ## header block)
    let sections: Vec<&str> = daily_md.split("\n## ").collect();
    if let Some(last) = sections.last() {
        let lines: Vec<&str> = last.lines().collect();
        if lines.len() > 1 {
            return lines[1..].join("\n").trim().to_string();
        }
    }
    String::new()
}

fn parse_summary_header(daily_md: &str) -> (usize, usize) {
    for line in daily_md.lines() {
        if line.starts_with("## ") && line.contains("messages") {
            let msg_count = line
                .split('(')
                .nth(1)
                .and_then(|s| s.split(" messages").next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let char_count = line
                .split(" messages, ")
                .nth(1)
                .and_then(|s| s.split(" chars").next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            return (msg_count, char_count);
        }
    }
    (0, 0)
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

[rocketchat.model]
default_provider = "mock"
default_model = "chat"
max_history_size = 12
max_text_length = 50000
max_iterations = 8
max_summary_chars = 8000
summary_days = 7

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

        let mut harness = AgentHarness::new(config, provider, None);
        let result = harness
            .process_message("room1", "general", "", false, "user", "Hi", &[])
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
            .process_message("dm-alice", "", "", true, "alice", "Hello bot", &[])
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
            .process_message("room1", "general", "", false, "user", "Hi", &[])
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

        let mut harness = AgentHarness::new(config, provider, None);

        let result = harness
            .process_message("room1", "general", "", false, "user", "search something", &[])
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

    #[tokio::test]
    async fn test_archive_room_if_needed_no_webdav() {
        let config = make_test_config();
        let provider = Box::new(MockProvider::new(vec![]));
        let mut harness = AgentHarness::new(config, provider, None);

        let room = harness
            .memory_mut()
            .get_or_create("room1", "general", "", false);
        for i in 0..10 {
            room.history.append(ChatMessage::user(format!("msg {}", i)));
        }

        let result = harness.archive_room_if_needed("room1").await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_and_archive_returns_seq() {
        let mut mgr = MemoryManager::new(50, 12, 8000, 7, 2000, 60, 30_000_000);
        let room = mgr.get_or_create("room1", "general", "", false);
        for i in 0..10 {
            room.history.append(ChatMessage::user(format!(
                "Message number {} with some padding text",
                i
            )));
        }

        let result = mgr.check_and_archive("room1");
        if let Some((rid, msgs)) = result {
            assert_eq!(rid, "room1");
            assert!(!msgs.is_empty());
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
        let result = inject_image_urls_from_refs(args, "general", "r-general", &refs);
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
        let result = inject_image_urls_from_refs(args, "general", "r-general", &refs);
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
        let result = inject_image_urls_from_refs(args, "general", "r-general", &refs);
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
}
