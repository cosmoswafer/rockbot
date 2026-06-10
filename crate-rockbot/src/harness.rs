use std::sync::Arc;

use tracing::{debug, error, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::AppConfig;
use crate::error::Result;
use crate::knowledge::KnowledgeManager;
use crate::memory::{DailySummary, MemoryJson, MemoryManager, MessageRef, SoulMemory};
use crate::provider::AiProvider;
use crate::tool::ToolRegistry;
use crate::types::{ChatMessage, ChatRequest, Role};

const DEFAULT_SYSTEM_PROMPT: &str = "\
You are RockBot, a helpful AI assistant running on a RocketChat server. \
You respond to DMs and @mentions concisely and helpfully. \
When you need the current date or time, use the datetime tool. \
When you need information from the web, use the web_search tool. \
When you need to fetch a URL, use web_fetch. \
When you need to analyze an image, use the vision tool. \
When you need to read, write, list, or manage files on remote storage, use the webdav tool. \
When you need to manage calendar events or todo tasks, use the calendar tool. \
When you need to generate an image, use the image_gen tool. \
When a user asks you to remember something, or you want to save permanent notes, \
preferences, or identity information, use the edit_soul tool. \
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
        let max_summary_chars = config.rocketchat.model.max_summary_chars;
        let summary_days = config.rocketchat.model.summary_days;
        let max_soul_chars = config.rocketchat.model.max_soul_chars;
        let config = Arc::new(config);
        Self {
            config,
            provider,
            memory: MemoryManager::new(max_chars, max_history, max_summary_chars, summary_days, max_soul_chars),
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
        room_fname: &str,
        is_dm: bool,
        sender_name: &str,
        text: &str,
    ) -> Result<Option<String>> {
        let clean_text = if !is_dm && !text.is_empty() {
            text.trim_start().to_string()
        } else {
            text.to_string()
        };

        let needs_restore = {
            let room = self.memory.get_or_create(room_id, room_name, room_fname, is_dm);
            room.history.messages.is_empty() && room.history.archive_seq == 0
        };

        if needs_restore && self.webdav.is_some() {
            self.restore_history(room_id, room_name, room_fname, is_dm).await;
        }

        let user_msg = ChatMessage::user(format!("{}: {}", sender_name, clean_text));
        if let Some(room) = self.memory.get_mut(room_id) {
            room.history.append(user_msg);
        }

        let system_prompt = self.build_system_prompt();
        let tool_defs = self.tools.definitions();
        let have_tools = !tool_defs.is_empty();

        let model = self.resolve_model();

        let wd = compute_webdav_dir(room_name, room_fname, is_dm);
        let _ = self.refresh_knowledge_context(room_id, &wd).await;

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
                                || tool_call.function.name == "edit_soul"
                                || tool_call.function.name == "save_knowledge"
                                || tool_call.function.name == "forget_knowledge"
                                || tool_call.function.name == "recall_knowledge"
                            {
                                let wd = compute_webdav_dir(room_name, room_fname, is_dm);
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

    pub async fn archive_room_if_needed(&mut self, room_id: &str) -> Result<()> {
        let needs_archive = self.memory.check_and_archive(room_id);
        if let Some((rid, msgs, seq)) = needs_archive {
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
                if let Err(e) = self.upsert_daily_summary(webdav_client, &wd, &summary, count, char_count).await {
                    warn!("Failed to write daily summary: {}", e);
                }

                // Legacy: write MemoryJson for backward compat
                let content = format_messages_as_json(&rid, seq, &summary, &msgs);
                let path = WebDavPath::new("").archive_path(&wd, seq);
                if let Err(e) = webdav_client
                    .write_file_with_fallback(&path, content.as_bytes().to_vec())
                    .await
                {
                    warn!("Failed to write memory archive for room {}: {}", rid, e);
                } else {
                    debug!(
                        "Archived {} messages for room {} to {} (daily summary written)",
                        count, rid, path
                    );
                }

                // Age out old summaries
                let summary_days = self.memory.summary_days;
                if let Err(e) = self.delete_old_summaries(webdav_client, &wd, summary_days).await {
                    warn!("Failed to clean up old summaries: {}", e);
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
        let _ = webdav.ensure_directory_all(&folder).await;

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
            Err(_) => return Ok(()),
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

    pub async fn load_archives_for_room(
        &self,
        _room_id: &str,
        room_name: &str,
        room_fname: &str,
        is_dm: bool,
    ) -> Result<Vec<MemoryJson>> {
        let webdav_client = match &self.webdav {
            Some(c) => c,
            None => return Ok(Vec::new()),
        };

        let wd = compute_webdav_dir(room_name, room_fname, is_dm);
        let mem_dir = WebDavPath::new("").memory_dir(&wd);

        if !webdav_client.exists(&mem_dir).await.unwrap_or(false) {
            return Ok(Vec::new());
        }

        let entries = webdav_client.list_directory(&mem_dir).await?;
        let mut json_files: Vec<&str> = entries
            .iter()
            .filter(|e| e.name.ends_with("_memory.json") && !e.is_dir)
            .map(|e| e.name.as_str())
            .collect();
        json_files.sort();

        let mut archives = Vec::new();
        for name in json_files.iter().take(5) {
            let path = format!("{}{}", mem_dir, name);
            match webdav_client.read_file_to_string(&path).await {
                Ok(content) => match serde_json::from_str::<MemoryJson>(&content) {
                    Ok(archive) => archives.push(archive),
                    Err(e) => {
                        warn!("Failed to parse memory archive {}: {}", path, e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read memory archive {}: {}", path, e);
                }
            }
        }

        Ok(archives)
    }

    pub async fn restore_history(
        &mut self,
        room_id: &str,
        room_name: &str,
        room_fname: &str,
        is_dm: bool,
    ) {
        let wd = compute_webdav_dir(room_name, room_fname, is_dm);

        // Layer 2: load daily summaries from WebDAV
        if let Some(ref webdav_client) = self.webdav {
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

        // Legacy: load MemoryJson archives for backward compat
        match self
            .load_archives_for_room(room_id, room_name, room_fname, is_dm)
            .await
        {
            Ok(archives) if !archives.is_empty() => {
                debug!(
                    "Restored {} legacy memory archives for room {}",
                    archives.len(),
                    room_name
                );
                self.memory
                    .restore_from_archives(room_id, room_name, room_fname, is_dm, &archives);
            }
            Ok(_) => {
                debug!("No legacy memory archives found for room {}", room_id);
            }
            Err(e) => {
                warn!(
                    "Failed to load legacy memory archives for room {}: {}",
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
            Err(_) => return Ok(Vec::new()),
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
            Err(_) => return Ok(SoulMemory {
                room_id: webdav_dir.to_string(),
                content: String::new(),
                updated_at: String::new(),
            }),
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
}

fn format_messages_as_json(
    room_id: &str,
    seq: u64,
    summary: &str,
    messages: &[ChatMessage],
) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let created_at = format!("{:?}", now);

    let date_range = {
        let first = messages.first().and_then(extract_msg_info);
        let last = messages.last().and_then(extract_msg_info);
        match (first, last) {
            (Some((_, _, t1)), Some((_, _, t2))) => format!("{} to {}", t1, t2),
            _ => "unknown".to_string(),
        }
    };

    let msg_refs: Vec<MessageRef> = messages
        .iter()
        .filter_map(|m| {
            let (id, author, timestamp) = extract_msg_info(m)?;
            let content = m.text_content()?.to_string();
            if content.is_empty() {
                None
            } else {
                Some(MessageRef {
                    id,
                    author,
                    content,
                    timestamp,
                })
            }
        })
        .collect();

    let archive = MemoryJson {
        schema: "rockbot-memory/1".into(),
        seq,
        room_id: room_id.to_string(),
        summary: summary.to_string(),
        date_range,
        msg_count: messages.len(),
        messages: msg_refs,
        created_at,
    };

    serde_json::to_string(&archive).unwrap_or_else(|_| "{}".into())
}

fn extract_msg_info(msg: &ChatMessage) -> Option<(String, String, String)> {
    let id = "".to_string();
    let timestamp = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{}", now)
    };
    let text = msg.text_content()?;
    let (author, _) = text.split_once(": ").unwrap_or((text, ""));
    Some((id, author.to_string(), timestamp))
}

fn inject_room_context(arguments: &str, room_id: &str, webdav_dir: &str) -> String {
    let mut args: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));
    args["room_id"] = serde_json::Value::String(room_id.to_string());
    args["webdav_dir"] = serde_json::Value::String(webdav_dir.to_string());
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

fn today_iso_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let days = (now.as_secs() / 86400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
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

fn now_iso_string() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time = secs % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
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
            .process_message("room1", "general", "", false, "user", "Hi")
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
            .process_message("dm-alice", "", "", true, "alice", "Hello bot")
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
            .process_message("room1", "general", "", false, "user", "Hi")
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
            .process_message("room1", "general", "", false, "user", "search something")
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
    fn test_format_messages_as_json() {
        let msgs = vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user("sender: Hello"),
            ChatMessage::assistant("Hi there!"),
        ];
        let json = format_messages_as_json("room1", 0, "Test summary", &msgs);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["schema"], "rockbot-memory/1");
        assert_eq!(parsed["seq"], 0);
        assert_eq!(parsed["room_id"], "room1");
        assert_eq!(parsed["summary"], "Test summary");
        assert_eq!(parsed["msg_count"], 3);
    }

    #[test]
    fn test_format_messages_as_json_empty_messages() {
        let msgs: Vec<ChatMessage> = vec![];
        let json = format_messages_as_json("room1", 5, "empty", &msgs);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["seq"], 5);
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_extract_msg_info() {
        let msg = ChatMessage::user("alice: hello world");
        let info = extract_msg_info(&msg);
        assert!(info.is_some());
        let (_, author, _) = info.unwrap();
        assert_eq!(author, "alice");
    }

    #[test]
    fn test_extract_msg_info_no_colon() {
        let msg = ChatMessage::assistant("plain reply");
        let info = extract_msg_info(&msg);
        assert!(info.is_some());
        let (_, author, _) = info.unwrap();
        assert_eq!(author, "plain reply");
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
        let mut mgr = MemoryManager::new(50, 12, 8000, 7, 2000);
        let room = mgr.get_or_create("room1", "general", "", false);
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
