use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use tracing::debug;

use crate::types::ChatMessage;
use crate::utils::{now_iso_string, today_iso_date};
use crate::validated::NonEmptyString;

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct PersistSnapshot {
    pub schema: NonEmptyString,
    pub room_id: NonEmptyString,
    pub messages: Vec<ChatMessage>,
    pub char_count: usize,
    /// Monotonic count of prune operations on this room's history. Empty
    /// `messages` together with `archive_seq > 0` is the durable marker of an
    /// explicit `!reset`; empty `messages` with `archive_seq == 0` means the
    /// room never had history.
    pub archive_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soul: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[validate(min_length = 1)]
    pub updated_at: String,
}

/// Max soul display-name length, counted in characters (not bytes).
pub const MAX_SOUL_NAME_CHARS: usize = 32;

/// Current soul schema version. Stamped by the writer, never authored by the
/// LLM — otherwise the model would copy a stale value forever.
pub const SOUL_SCHEMA_VERSION: u32 = 2;

/// Invariant newtype: a valid identity name is non-empty, single-line,
/// control-char free and <= `MAX_SOUL_NAME_CHARS` characters. Holding one
/// proves the invariant, so callers never re-validate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoulName(String);

impl SoulName {
    pub fn try_new(value: &str) -> std::result::Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("name must not be empty".into());
        }
        if trimmed.contains(['\n', '\r']) {
            return Err("name must be a single line".into());
        }
        if trimmed.chars().any(char::is_control) {
            return Err("name must not contain control characters".into());
        }
        if trimmed.chars().count() > MAX_SOUL_NAME_CHARS {
            return Err(format!("name must be <= {MAX_SOUL_NAME_CHARS} chars"));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Flat YAML-subset identity block. `extra` preserves unknown keys so a
/// hand-written `persona_version:` survives a bot rewrite.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoulFrontMatter {
    pub name: Option<SoulName>,
    pub pronouns: Option<String>,
    pub address_user_as: Option<String>,
    pub language: Option<String>,
    pub schema: Option<u32>,
    pub updated: Option<String>,
    pub extra: Vec<(String, String)>,
}

impl SoulFrontMatter {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.pronouns.is_none()
            && self.address_user_as.is_none()
            && self.language.is_none()
            && self.schema.is_none()
            && self.updated.is_none()
            && self.extra.is_empty()
    }

    /// Prompt-facing one-liner. Deliberately short; the body carries the prose.
    pub fn identity_line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(name) = &self.name {
            parts.push(format!("name: {}", name.as_str()));
        }
        if let Some(lang) = &self.language {
            parts.push(format!("language: {lang}"));
        }
        if let Some(address) = &self.address_user_as {
            parts.push(format!("address user as: {address}"));
        }
        (!parts.is_empty()).then(|| format!("[Identity — {}]", parts.join(", ")))
    }
}

/// Parsed soul document: identity metadata + prompt body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoulDocument {
    pub front: SoulFrontMatter,
    pub body: String,
    /// A well-formed `---` block was found.
    pub had_front_matter: bool,
    /// `---` opened the file but the block did not parse; `body` then holds
    /// the entire raw document so nothing is lost.
    pub broken_front_matter: Option<String>,
}

impl SoulDocument {
    /// Total function: never fails. Broken front matter degrades to a legacy
    /// document (raw text kept as body).
    pub fn parse(raw: &str) -> Self {
        let normalized = normalize_newlines(raw);
        let text = normalized.as_ref();

        let Some((block, rest)) = split_front_matter(text) else {
            return Self::legacy(text);
        };
        match parse_front_matter_block(block) {
            Ok(front) if !front.is_empty() => Self {
                front,
                body: rest.trim_start_matches('\n').to_string(),
                had_front_matter: true,
                broken_front_matter: None,
            },
            // `---` was a horizontal rule, or the block is empty: plain body.
            Ok(_) => Self::legacy(text),
            Err(reason) => Self {
                front: SoulFrontMatter::default(),
                body: text.to_string(),
                had_front_matter: false,
                broken_front_matter: Some(reason),
            },
        }
    }

    fn legacy(text: &str) -> Self {
        Self {
            front: SoulFrontMatter::default(),
            body: text.to_string(),
            had_front_matter: false,
            broken_front_matter: None,
        }
    }

    /// Fill in a name the LLM dropped and stamp schema/updated before a write.
    pub fn normalized_for_write(mut self, previous: Option<&SoulName>) -> Self {
        if self.broken_front_matter.is_some() {
            return self;
        }
        if self.front.name.is_none() {
            self.front.name = extract_identity_name(&self.body)
                .and_then(|raw| SoulName::try_new(&raw).ok())
                .or_else(|| previous.cloned());
        }
        // Legacy document: move the identity bullet into the front matter so a
        // single edit round-trips to the new format in one step.
        if !self.had_front_matter {
            if let Some(name) = self.front.name.clone() {
                self.body = strip_identity_bullet(&self.body, name.as_str());
            }
        }
        if self.had_front_matter || self.front.name.is_some() {
            self.front.schema = Some(SOUL_SCHEMA_VERSION);
            self.front.updated = Some(today_iso_date());
        }
        self
    }

    /// Canonical serialization. Emits front matter only when one will exist,
    /// so a legacy soul with no identity is written back verbatim.
    pub fn render(&self) -> String {
        if self.broken_front_matter.is_some() || self.front.is_empty() {
            return self.body.clone();
        }
        let mut out = String::from("---\n");
        if let Some(name) = &self.front.name {
            out.push_str(&format!("name: {}\n", yaml_scalar(name.as_str())));
        }
        for (key, value) in [
            ("pronouns", &self.front.pronouns),
            ("address_user_as", &self.front.address_user_as),
            ("language", &self.front.language),
        ] {
            if let Some(value) = value {
                out.push_str(&format!("{key}: {}\n", yaml_scalar(value)));
            }
        }
        if let Some(schema) = self.front.schema {
            out.push_str(&format!("schema: {schema}\n"));
        }
        if let Some(updated) = &self.front.updated {
            out.push_str(&format!("updated: {}\n", yaml_scalar(updated)));
        }
        for (key, value) in &self.front.extra {
            out.push_str(&format!("{key}: {}\n", yaml_scalar(value)));
        }
        out.push_str("---\n\n");
        out.push_str(&self.body);
        out
    }
}

fn normalize_newlines(raw: &str) -> std::borrow::Cow<'_, str> {
    let body = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if body.contains('\r') {
        std::borrow::Cow::Owned(body.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(body)
    }
}

/// Only a block at byte 0 counts; a `---` used as a horizontal rule later in
/// the body is never front matter.
fn split_front_matter(text: &str) -> Option<(&str, &str)> {
    let first = text.split_inclusive('\n').next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let block_start = first.len();
    if block_start >= text.len() {
        return None;
    }
    let mut offset = block_start;
    for line in text[block_start..].split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            return Some((&text[block_start..offset], &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    None // unterminated block -> legacy body
}

fn parse_front_matter_block(block: &str) -> std::result::Result<SoulFrontMatter, String> {
    let mut front = SoulFrontMatter::default();
    for (idx, line) in block.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("line {} is not `key: value`", idx + 1));
        };
        let key = key.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err(format!("invalid key on line {}", idx + 1));
        }
        let value = unquote_yaml(value.trim());
        match key {
            "name" => {
                front.name = Some(SoulName::try_new(&value)?);
            }
            "pronouns" => front.pronouns = non_empty_string(value),
            "address_user_as" => front.address_user_as = non_empty_string(value),
            "language" => front.language = non_empty_string(value),
            "schema" => {
                front.schema =
                    Some(value.parse::<u32>().map_err(|e| format!("schema: {e}"))?);
            }
            "updated" => front.updated = non_empty_string(value),
            other => front.extra.push((other.to_string(), value)),
        }
    }
    Ok(front)
}

fn non_empty_string(value: String) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_string())
}

fn unquote_yaml(value: &str) -> String {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

/// Quote only when the value could be misread as YAML structure.
fn yaml_scalar(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"',
            '%', '@', '`',
        ])
        || value.contains(": ")
        || value.contains(" #")
        || value.contains('\n');
    if needs_quotes {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct SoulMemory {
    pub room_id: NonEmptyString,
    /// Raw file contents — used for logging, snapshots, and round-trip writes.
    pub content: String,
    pub front: SoulFrontMatter,
    /// Prompt-facing markdown with the front matter removed.
    pub body: String,
    /// True only for genuine front matter (legacy derived names stay false).
    pub had_front_matter: bool,
    /// Set when the leading `---` block failed to parse (raw kept as `body`).
    pub broken_front_matter: Option<String>,
    pub updated_at: String,
}

impl SoulMemory {
    pub fn from_raw(room_id: NonEmptyString, raw: String, updated_at: String) -> Self {
        let mut doc = SoulDocument::parse(&raw);
        // Backward compat: derive the name from the legacy `- My name is X`
        // bullet once, at load time, so every consumer sees the same value.
        if doc.front.name.is_none() && doc.broken_front_matter.is_none() {
            doc.front.name = extract_identity_name(&doc.body)
                .and_then(|raw| SoulName::try_new(&raw).ok());
        }
        Self {
            room_id,
            content: raw,
            front: doc.front,
            body: doc.body,
            had_front_matter: doc.had_front_matter,
            broken_front_matter: doc.broken_front_matter,
            updated_at,
        }
    }

    /// Legacy file, parseable, and a name was derived from the body.
    pub fn needs_migration(&self) -> bool {
        !self.had_front_matter
            && self.broken_front_matter.is_none()
            && self.front.name.is_some()
    }

    /// Move identity out of the bullet list into front matter.
    pub fn migrated(self) -> SoulDocument {
        let mut doc = SoulDocument {
            front: self.front,
            body: self.body,
            had_front_matter: true,
            broken_front_matter: None,
        };
        if let Some(name) = doc.front.name.clone() {
            doc.body = strip_identity_bullet(&doc.body, name.as_str());
        }
        doc.normalized_for_write(None)
    }
}


#[derive(Debug, Clone)]
pub struct ConversationHistory {
    pub room_id: String,
    pub messages: Vec<ChatMessage>,
    pub char_count: usize,
    pub archive_seq: u64,
}

impl ConversationHistory {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            messages: Vec::new(),
            char_count: 0,
            archive_seq: 0,
        }
    }

    pub fn append(&mut self, msg: ChatMessage) {
        if let Some(text) = msg.text_content() {
            self.char_count += text.chars().count();
        }
        self.messages.push(msg);
    }

    pub fn needs_archive(&self, max_chars: usize) -> bool {
        self.char_count > max_chars && self.messages.len() > 4
    }

    pub fn oldest_messages(&self, count: usize) -> &[ChatMessage] {
        let len = self.messages.len();
        let take = count.min(len);
        &self.messages[..take]
    }

    pub fn prune_first(&mut self, count: usize) {
        let len = self.messages.len();
        let remove = count.min(len);
        let removed: Vec<_> = self.messages.drain(..remove).collect();
        for msg in &removed {
            if let Some(text) = msg.text_content() {
                self.char_count = self.char_count.saturating_sub(text.chars().count());
            }
        }
        self.archive_seq += 1;
    }
}

#[derive(Debug, Clone)]
pub struct RoomState {
    pub room_id: String,
    pub room_name: String,
    pub room_fname: String,
    pub is_dm: bool,
    pub history: ConversationHistory,
    pub last_activity: u64,
}

impl RoomState {
    pub fn new(
        room_id: impl Into<String>,
        room_name: impl Into<String>,
        room_fname: impl Into<String>,
        is_dm: bool,
    ) -> Self {
        let room_id = room_id.into();
        Self {
            history: ConversationHistory::new(&room_id),
            room_id,
            room_name: room_name.into(),
            room_fname: room_fname.into(),
            is_dm,
            last_activity: 0,
        }
    }

    pub fn touch(&mut self) {
        use std::time::{SystemTime, UNIX_EPOCH};
        self.last_activity = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

#[derive(Debug)]
pub struct MemoryManager {
    rooms: HashMap<String, RoomState>,
    pub max_soul_chars: usize,
    max_context_bytes: usize,
    souls: HashMap<String, SoulMemory>,
    knowledge: HashMap<String, String>,
    dirty_snapshots: HashSet<String>,
    /// Rooms whose legacy soul migration has already been attempted (once per
    /// process, so a read-only WebDAV deployment does not retry every message).
    soul_migration_attempted: HashSet<String>,
    pub persist_interval_secs: u64,
    /// Per-room flag: token pressure detected (set during LLM call, checked after reply)
    token_pressure: HashSet<String>,
    /// Per-room flag: byte pressure detected (set during context assembly, checked after reply)
    byte_pressure: HashSet<String>,
    /// Per-room flag: user explicitly requested reset (set during process_message, checked after reply)
    explicit_reset: HashSet<String>,
}

/// Hardcoded max messages in context window (replaces configurable max_history_size)
const MAX_CONTEXT_MESSAGES: usize = 30;

impl MemoryManager {
    pub fn new(
        max_soul_chars: usize,
        persist_interval_secs: u64,
        max_context_bytes: usize,
    ) -> Self {
        Self {
            rooms: HashMap::new(),
            max_soul_chars,
            max_context_bytes,
            souls: HashMap::new(),
            knowledge: HashMap::new(),
            dirty_snapshots: HashSet::new(),
            soul_migration_attempted: HashSet::new(),
            persist_interval_secs,
            token_pressure: HashSet::new(),
            byte_pressure: HashSet::new(),
            explicit_reset: HashSet::new(),
        }
    }

    pub fn get_or_create(
        &mut self,
        room_id: &str,
        room_name: &str,
        room_fname: &str,
        is_dm: bool,
    ) -> &mut RoomState {
        self.rooms.entry(room_id.to_string()).or_insert_with(|| {
            RoomState::new(
                room_id.to_string(),
                room_name.to_string(),
                room_fname.to_string(),
                is_dm,
            )
        })
    }

    pub fn get(&self, room_id: &str) -> Option<&RoomState> {
        self.rooms.get(room_id)
    }

    pub fn get_mut(&mut self, room_id: &str) -> Option<&mut RoomState> {
        self.rooms.get_mut(room_id)
    }

    pub fn check_and_archive(&mut self, room_id: &str, force: bool) -> Option<(String, Vec<ChatMessage>)> {
        let room = self.rooms.get(room_id)?;
        if room.history.messages.len() <= 4 && !force {
            return None;
        }
        let take_count = room.history.messages.len();
        if take_count == 0 {
            return None;
        }
        let to_archive = room.history.oldest_messages(take_count);
        let messages: Vec<ChatMessage> = to_archive.to_vec();
        let room_id = room.room_id.clone();
        Some((room_id, messages))
    }

    pub fn set_token_pressure(&mut self, room_id: &str) {
        self.token_pressure.insert(room_id.to_string());
    }

    pub fn set_byte_pressure(&mut self, room_id: &str) {
        self.byte_pressure.insert(room_id.to_string());
    }

    pub fn has_token_pressure(&self, room_id: &str) -> bool {
        self.token_pressure.contains(room_id)
    }

    pub fn has_byte_pressure(&self, room_id: &str) -> bool {
        self.byte_pressure.contains(room_id)
    }

    pub fn set_explicit_reset(&mut self, room_id: &str) {
        self.explicit_reset.insert(room_id.to_string());
    }

    pub fn has_explicit_reset(&self, room_id: &str) -> bool {
        self.explicit_reset.contains(room_id)
    }

    pub fn needs_reset(&self, room_id: &str) -> bool {
        self.has_token_pressure(room_id)
            || self.has_byte_pressure(room_id)
            || self.has_explicit_reset(room_id)
    }

    pub fn clear_pressure_flags(&mut self, room_id: &str) {
        self.token_pressure.remove(room_id);
        self.byte_pressure.remove(room_id);
        self.explicit_reset.remove(room_id);
    }

    pub fn prune_archived(&mut self, room_id: &str, count: usize) {
        if let Some(room) = self.rooms.get_mut(room_id) {
            room.history.prune_first(count);
        }
    }

    pub fn message_count(&self, room_id: &str) -> usize {
        self.rooms
            .get(room_id)
            .map(|r| r.history.messages.len())
            .unwrap_or(0)
    }

    pub fn oldest_messages(&self, room_id: &str, count: usize) -> Vec<ChatMessage> {
        self.rooms
            .get(room_id)
            .map(|r| r.history.oldest_messages(count).to_vec())
            .unwrap_or_default()
    }

    pub fn summarize_room(
        &mut self,
        room_id: &str,
        summarize_count: usize,
        summary_msg: ChatMessage,
    ) {
        if let Some(room) = self.rooms.get_mut(room_id) {
            room.history.prune_first(summarize_count);
            room.history.messages.insert(0, summary_msg);
            room.history.char_count = room
                .history
                .messages
                .iter()
                .filter_map(|m| m.text_content())
                .map(|t| t.chars().count())
                .sum();
            self.mark_snapshot_dirty(room_id);
        }
    }

    pub fn strip_half(&mut self, room_id: &str) -> usize {
        if let Some(room) = self.rooms.get_mut(room_id) {
            let half = room.history.messages.len() / 2;
            if half > 0 {
                room.history.prune_first(half);
                self.mark_snapshot_dirty(room_id);
            }
            half
        } else {
            0
        }
    }

    pub fn build_context(
        &self,
        room_id: &str,
        system_prompt: &str,
        max_history: Option<usize>,
        extra_context: Option<Vec<ChatMessage>>,
    ) -> Vec<ChatMessage> {
        let limit = max_history.unwrap_or(MAX_CONTEXT_MESSAGES);
        // Single leading system message invariant: all system-role content
        // (system prompt, soul, knowledge index, leading conversation summary)
        // is merged into ONE system message at index 0, joined by "\n\n".
        // Strict chat templates (Qwen3.5/3.6-derived, e.g. Bonsai-27B) reject
        // any system message at index >= 1 with HTTP 400 — Gitea issue #77.
        let mut system_parts: Vec<String> = vec![system_prompt.to_string()];
        let mut has_soul = false;
        let mut has_knowledge = false;

        if let Some(soul) = self.souls.get(room_id) {
            let body = soul.body.trim();
            // The derived identity line is only injected for real front matter;
            // legacy souls already carry `- My name is ...` in the body.
            // Suppress it when the body still states the same name (migration
            // could not remove the bullet because it carried extra prose).
            let identity = soul
                .front
                .identity_line()
                .filter(|_| soul.had_front_matter)
                .filter(|_| !body_declares_name(body, soul.front.name.as_ref()));
            if !body.is_empty() || identity.is_some() {
                has_soul = true;
                let truncated = truncate_chars(body, self.max_soul_chars);
                let block = match (identity, truncated.is_empty()) {
                    (Some(identity), false) => format!("{identity}\n{truncated}"),
                    (Some(identity), true) => identity,
                    (None, _) => truncated,
                };
                system_parts.push(format!(
                    "[Core memory — permanent preferences, identity, and facts]\n{block}"
                ));
            }
        }

        if let Some(knowledge_text) = self.knowledge.get(room_id) {
            if !knowledge_text.is_empty() {
                has_knowledge = true;
                system_parts.push(knowledge_text.clone());
            }
        }

        // Absorb leading system messages from history (e.g. the
        // conversation summary inserted by summarize_room) into the
        // merged system prefix instead of emitting them separately.
        let history_start = self.rooms.get(room_id).map(|room| {
            let history = &room.history.messages;
            let start = if history.len() > limit {
                history.len() - limit
            } else {
                0
            };
            let mut lead_end = start;
            while lead_end < history.len()
                && history[lead_end].role == crate::types::Role::System
            {
                if let Some(text) = history[lead_end].text_content() {
                    if !text.is_empty() {
                        system_parts.push(text.to_string());
                    }
                }
                lead_end += 1;
            }
            lead_end
        });

        let mut messages = vec![ChatMessage::system(system_parts.join("\n\n"))];

        if let (Some(room), Some(start)) = (self.rooms.get(room_id), history_start) {
            let slice = &room.history.messages[start..];
            // Find the index of the last user message — preserve its images.
            let last_user_idx = slice
                .iter()
                .rposition(|m| m.role == crate::types::Role::User);
            for (i, msg) in slice.iter().enumerate() {
                let msg = if Some(i) == last_user_idx {
                    msg.clone()
                } else {
                    strip_images_from_message(msg.clone())
                };
                messages.push(msg);
            }
        }

        if let Some(extra) = extra_context {
            messages.splice(1..1, extra);
        }

        strip_orphaned_tool_calls(&mut messages);

        let history_count = messages.iter().filter(|m| m.role != crate::types::Role::System).count();
        debug!(
            "build_context room={}: total={} system_msgs={} history_msgs={} soul={} knowledge={}",
            room_id, messages.len(),
            messages.iter().filter(|m| m.role == crate::types::Role::System).count(),
            history_count,
            has_soul, has_knowledge,
        );

        // Enforce max_context_bytes: drop oldest images until under limit.
        if self.max_context_bytes > 0 {
            // Fast guard: if char_count + generous JSON overhead is under the limit,
            // skip expensive per-message serialization entirely.
            let char_count = self.rooms.get(room_id).map(|r| r.history.char_count).unwrap_or(0);
            let estimated_bytes = char_count + (200 * messages.len());
            if estimated_bytes <= self.max_context_bytes {
                return messages;
            }
            let mut total = messages
                .iter()
                .map(|m| count_json_bytes(m))
                .sum::<usize>();
            if total > self.max_context_bytes {
                debug!(
                    "build_context room={}: context exceeds max_context_bytes ({} > {}), stripping images",
                    room_id, total, self.max_context_bytes
                );
                // Walk from oldest to newest, stripping images until under limit.
                // The last user message is preserved (the current request).
                let last_user_idx = messages.iter().rposition(|m| m.role == crate::types::Role::User);
                #[allow(clippy::needless_range_loop)]
                for i in 0..messages.len() {
                    if total <= self.max_context_bytes {
                        break;
                    }
                    if Some(i) == last_user_idx {
                        continue; // preserve latest user message with images
                    }
                    let before = count_json_bytes(&messages[i]);
                    messages[i] = strip_images_from_message(messages[i].clone());
                    let after = count_json_bytes(&messages[i]);
                    total = total.saturating_sub(before.saturating_sub(after));
                }
                debug!(
                    "build_context room={}: after stripping images: {} bytes",
                    room_id, total
                );
            }
        }

        messages
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    pub fn set_soul(&mut self, room_id: &str, soul: SoulMemory) {
        self.souls.insert(room_id.to_string(), soul);
    }

    pub fn get_soul(&self, room_id: &str) -> Option<&SoulMemory> {
        self.souls.get(room_id)
    }

    /// Effective identity name for a room: front-matter `name`, or the legacy
    /// `- My name is X` bullet derived at load time.
    pub fn soul_name(&self, room_id: &str) -> Option<&str> {
        self.souls.get(room_id)?.front.name.as_ref().map(SoulName::as_str)
    }

    pub fn self_display_name(&self, room_id: &str) -> Option<String> {
        let name = self.soul_name(room_id);
        match name {
            Some(name) => debug!("self_display_name: room={} name={:?}", room_id, name),
            None => debug!("self_display_name: no display name found for room={}", room_id),
        }
        name.map(str::to_string)
    }

    /// Returns true only the first time a room's legacy soul migration is
    /// attempted in this process.
    pub fn claim_soul_migration(&mut self, room_id: &str) -> bool {
        self.soul_migration_attempted.insert(room_id.to_string())
    }

    /// Returns the first available self-display name from any room's soul memory.
    pub fn any_display_name(&self) -> Option<String> {
        for room_id in self.souls.keys() {
            if let Some(name) = self.self_display_name(room_id) {
                return Some(name);
            }
        }
        None
    }

    pub fn set_knowledge(&mut self, room_id: &str, entries: String) {
        self.knowledge.insert(room_id.to_string(), entries);
    }

    pub fn get_knowledge(&self, room_id: &str) -> Option<&str> {
        self.knowledge.get(room_id).map(|s| s.as_str())
    }

    pub fn mark_snapshot_dirty(&mut self, room_id: &str) {
        debug!("mark_snapshot_dirty: {}", room_id);
        self.dirty_snapshots.insert(room_id.to_string());
    }

    pub fn dirty_snapshots(&self) -> Vec<String> {
        self.dirty_snapshots.iter().cloned().collect()
    }

    pub fn clear_dirty(&mut self, room_id: &str) {
        self.dirty_snapshots.remove(room_id);
    }

    pub fn build_snapshot(&self, room_id: &str) -> Option<PersistSnapshot> {
        let room = self.rooms.get(room_id)?;
        let updated_at = now_iso_string();

        let mut snapshot = PersistSnapshot {
            schema: NonEmptyString::try_new("rockbot-snapshot/1".to_string()).expect("hardcoded"),
            room_id: NonEmptyString::try_new(room_id.to_string()).expect("room_id must be non-empty"),
            messages: room.history.messages.clone(),
            char_count: room.history.char_count,
            archive_seq: room.history.archive_seq,
            soul: None,
            summary: None,
            updated_at,
        };

        if let Some(soul) = self.souls.get(room_id) {
            if !soul.content.is_empty() {
                snapshot.soul = Some(soul.content.clone());
            }
        }

        Some(snapshot)
    }

    pub fn restore_snapshot(&mut self, snapshot: &PersistSnapshot) {
        if let Some(room) = self.rooms.get_mut(snapshot.room_id.as_str()) {
            let mut messages = snapshot.messages.clone();
            strip_orphaned_tool_calls(&mut messages);
            // Recompute char_count from cleaned messages
            let char_count: usize = messages.iter().filter_map(|m| m.text_content()).map(|t| t.chars().count()).sum();
            room.history.messages = messages;
            room.history.char_count = char_count;
            room.history.archive_seq = snapshot.archive_seq;
        }

        if let Some(ref soul_content) = snapshot.soul {
            let soul = SoulMemory::from_raw(
                snapshot.room_id.clone(),
                soul_content.clone(),
                snapshot.updated_at.clone(),
            );
            let slen = soul_content.chars().count();
            let stail: String = soul_content.chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect();
            let room_key: String = snapshot.room_id.to_string();
            debug!(
                "restore_snapshot: soul restored for room {} (len={} chars, tail={:?})",
                room_key, slen, stail
            );
            self.souls.insert(room_key, soul);
        }
    }

    pub fn room_ids(&self) -> Vec<String> {
        self.rooms.keys().cloned().collect()
    }

    pub fn evict_room(&mut self, room_id: &str) -> Option<RoomState> {
        self.souls.remove(room_id);
        self.knowledge.remove(room_id);
        self.dirty_snapshots.remove(room_id);
        self.soul_migration_attempted.remove(room_id);
        self.token_pressure.remove(room_id);
        self.byte_pressure.remove(room_id);
        self.explicit_reset.remove(room_id);
        self.rooms.remove(room_id)
    }

    pub fn stale_rooms(&self, ttl_secs: u64) -> Vec<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.rooms
            .iter()
            .filter(|(_, room)| {
                !room.history.messages.is_empty()
                    && room.last_activity > 0
                    && now.saturating_sub(room.last_activity) > ttl_secs
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn touch_room(&mut self, room_id: &str) {
        if let Some(room) = self.rooms.get_mut(room_id) {
            room.touch();
        }
    }
}

pub(crate) fn truncate_message_content(msg: &mut ChatMessage, max_chars: usize) {
    match &mut msg.content {
        crate::types::MessageContent::Text(text) => {
            if text.chars().count() > max_chars {
                let mut chars: Vec<char> = text.chars().take(max_chars).collect();
                chars.extend("\n\n[...truncated]".chars());
                *text = chars.into_iter().collect();
            }
        }
        crate::types::MessageContent::Multipart(parts) => {
            for part in parts.iter_mut() {
                if let crate::types::ContentPart::Text { text } = part {
                    if text.chars().count() > max_chars {
                        let mut chars: Vec<char> = text.chars().take(max_chars).collect();
                        chars.extend("\n\n[...truncated]".chars());
                        *text = chars.into_iter().collect();
                    }
                }
            }
        }
    }
}

pub(crate) fn strip_images_from_message(msg: ChatMessage) -> ChatMessage {
    let crate::types::MessageContent::Multipart(ref parts) = msg.content else {
        return msg;
    };
    let has_image = parts.iter().any(|p| matches!(p, crate::types::ContentPart::ImageUrl { .. }));
    if !has_image {
        return msg;
    }
    let text = parts
        .iter()
        .map(|p| match p {
            crate::types::ContentPart::Text { text } => text.as_str(),
            crate::types::ContentPart::ImageUrl { .. } => "[image]",
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut stripped = msg;
    stripped.content = crate::types::MessageContent::Text(text);
    stripped
}

pub(crate) fn strip_orphaned_tool_calls(messages: &mut Vec<ChatMessage>) {
    // Phase 1: remove orphaned tool messages that lack a preceding
    // assistant with tool_calls. This happens when history truncation
    // cuts off the assistant but leaves its tool results behind.
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role == crate::types::Role::Tool {
            let mut preceded_by_tool_calls = false;
            let mut j = i;
            while j > 0 {
                j -= 1;
                if messages[j].role == crate::types::Role::Assistant
                    && messages[j].tool_calls.is_some()
                {
                    preceded_by_tool_calls = true;
                    break;
                }
                if messages[j].role != crate::types::Role::Tool {
                    break;
                }
            }
            if !preceded_by_tool_calls {
                messages.remove(i);
                continue;
            }
        }
        i += 1;
    }

    // Phase 2: remove assistant tool_calls that have no tool replies
    // (e.g. the AI's last message was a tool call that never completed
    // before the loop was interrupted, or a tool execution failed and
    // later messages were appended after it).
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role == crate::types::Role::Assistant && messages[i].tool_calls.is_some() {
            // Collect expected tool_call_ids
            let expected_ids: Vec<String> = messages[i]
                .tool_calls
                .as_ref()
                .map(|tcs| tcs.iter().map(|tc| tc.id.clone()).collect())
                .unwrap_or_default();
            let mut matched = HashSet::new();
            for item in messages.iter().skip(i + 1) {
                if item.role == crate::types::Role::Tool {
                    if let Some(ref id) = item.tool_call_id {
                        if expected_ids.contains(id) {
                            matched.insert(id.clone());
                        }
                    }
                } else {
                    break;
                }
            }
            if matched.len() != expected_ids.len() {
                // Remove this assistant
                messages.remove(i);
                // Remove orphaned tool results that followed it
                while i < messages.len() && messages[i].role == crate::types::Role::Tool {
                    messages.remove(i);
                }
                continue;
            }
        }
        i += 1;
    }
}

/// Extract display name from soul content using a standard regex.
///
/// Regex: `My name is (.+)` — captures the display name from the
/// legacy flat enumeration list (`- My name is ...`). This is only the
/// backward-compatible fallback; front-matter `name` is authoritative.
fn extract_identity_name(content: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"My name is (.+)").expect("hardcoded regex")
    });
    let caps = RE.captures(content)?;
    let name = caps.get(1)?.as_str().trim().to_string();
    if !name.is_empty() && name.chars().count() <= MAX_SOUL_NAME_CHARS {
        Some(name)
    } else {
        None
    }
}

/// Drop only the exact `- My name is <name>` line. A bullet carrying extra
/// prose is kept (nothing is silently deleted); the identity line is then
/// suppressed by the dedupe rule in `build_context`.
fn strip_identity_bullet(body: &str, name: &str) -> String {
    let marker = format!("My name is {name}");
    let mut removed = false;
    body.split_inclusive('\n')
        .filter(|line| {
            let trimmed = line.trim();
            let is_bullet = trimmed.starts_with("- ") || trimmed.starts_with("* ");
            if !removed && is_bullet && trimmed[2..].trim() == marker {
                removed = true;
                return false;
            }
            true
        })
        .collect()
}

fn body_declares_name(body: &str, name: Option<&SoulName>) -> bool {
    name.is_some_and(|name| body.contains(&format!("My name is {}", name.as_str())))
}

/// Truncate to `max` characters and append the marker.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max).collect();
    truncated.push_str("\n\n[truncated]");
    truncated
}

struct ByteCounter(usize);

impl Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[inline]
pub(crate) fn count_json_bytes(msg: &ChatMessage) -> usize {
    let mut counter = ByteCounter(0);
    let _ = serde_json::to_writer(&mut counter, msg);
    counter.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Role;

    fn make_msg(role: Role, text: &str) -> ChatMessage {
        match role {
            Role::User => ChatMessage::user(text),
            Role::Assistant => ChatMessage::assistant(text),
            Role::System => ChatMessage::system(text),
            Role::Tool => ChatMessage::tool("id", text),
        }
    }

    #[test]
    fn test_conversation_history_new() {
        let history = ConversationHistory::new("room1");
        assert_eq!(history.room_id, "room1");
        assert_eq!(history.messages.len(), 0);
        assert_eq!(history.char_count, 0);
        assert_eq!(history.archive_seq, 0);
    }

    #[test]
    fn test_append_message() {
        let mut history = ConversationHistory::new("room1");
        history.append(make_msg(Role::User, "Hello world"));
        assert_eq!(history.messages.len(), 1);
        assert_eq!(history.char_count, 11);
    }

    #[test]
    fn test_needs_archive() {
        let mut history = ConversationHistory::new("room1");
        history.append(make_msg(Role::User, "Short"));
        assert!(!history.needs_archive(100));

        for i in 0..10 {
            history.append(make_msg(Role::User, &format!("msg number {}", i)));
        }
        assert!(history.needs_archive(50));
    }

    #[test]
    fn test_needs_archive_too_few_messages() {
        let mut history = ConversationHistory::new("room1");
        history.append(make_msg(Role::User, "A very long message that exceeds the character limit but there are not enough messages"));
        assert!(!history.needs_archive(10));
    }

    #[test]
    fn test_oldest_messages() {
        let mut history = ConversationHistory::new("room1");
        for i in 0..5 {
            history.append(make_msg(Role::User, &format!("msg {}", i)));
        }
        let oldest = history.oldest_messages(2);
        assert_eq!(oldest.len(), 2);
        assert_eq!(oldest[0].text_content(), Some("msg 0"));
        assert_eq!(oldest[1].text_content(), Some("msg 1"));
    }

    #[test]
    fn test_prune_first() {
        let mut history = ConversationHistory::new("room1");
        for i in 0..5 {
            history.append(make_msg(Role::User, &format!("msg {}", i)));
        }
        let initial_count = history.char_count;
        history.prune_first(2);
        assert_eq!(history.messages.len(), 3);
        assert!(history.char_count < initial_count);
        assert_eq!(history.archive_seq, 1);
        assert_eq!(history.messages[0].text_content(), Some("msg 2"));
    }

    #[test]
    fn test_prune_first_all() {
        let mut history = ConversationHistory::new("room1");
        history.append(make_msg(Role::User, "test"));
        history.prune_first(10);
        assert_eq!(history.messages.len(), 0);
        assert_eq!(history.char_count, 0);
    }

    #[test]
    fn test_memory_manager_get_or_create() {
        let mut mgr = MemoryManager::new(2000, 60, 30_000_000);
        let room = mgr.get_or_create("room1", "general", "", false);
        assert_eq!(room.room_id, "room1");
        assert_eq!(room.room_name, "general");
        assert_eq!(room.room_fname, "");
        assert!(!room.is_dm);

        let room2 = mgr.get_or_create("room1", "other", "fname_other", true);
        assert_eq!(room2.room_name, "general");
        assert_eq!(room2.room_fname, "");
    }

    #[test]
    fn test_memory_manager_build_context() {
        let mut mgr = MemoryManager::new(2000, 60, 30_000_000);
        let room = mgr.get_or_create("room1", "general", "", false);
        for i in 0..5 {
            room.history
                .append(make_msg(Role::User, &format!("msg {}", i)));
        }

        let ctx = mgr.build_context("room1", "You are helpful", None, None);
        assert_eq!(ctx.len(), 6, "1 system + 5 history (max is 30)");
        assert_eq!(ctx[0].role, Role::System);
        assert_eq!(ctx[1].text_content(), Some("msg 0"));
        assert_eq!(ctx[5].text_content(), Some("msg 4"));
    }

    #[test]
    fn test_memory_manager_build_context_nonexistent_room() {
        let mgr = MemoryManager::new(2000, 60, 30_000_000);
        let ctx = mgr.build_context("nonexistent", "prompt", None, None);
        assert_eq!(ctx.len(), 1);
    }

    #[test]
    fn test_build_context_merges_system_content_into_single_message() {
        // Gitea #77: strict chat templates reject any system message at
        // index >= 1. Soul + knowledge + prompt must merge into ONE
        // leading system message.
        let mut mgr = MemoryManager::new(2000, 60, 30_000_000);
        let room = mgr.get_or_create("room1", "general", "", false);
        room.history.append(make_msg(Role::User, "hello"));

        let soul = SoulMemory::from_raw(
            NonEmptyString::try_new("room1".to_string()).unwrap(),
            "- My name is TestBot".into(),
            String::new(),
        );
        mgr.set_soul("room1", soul);
        mgr.set_knowledge("room1", "[Knowledge Index]\n[P1] note — stuff".to_string());

        let ctx = mgr.build_context("room1", "You are helpful", None, None);
        let system_count = ctx.iter().filter(|m| m.role == Role::System).count();
        assert_eq!(system_count, 1, "exactly one leading system message expected");
        assert_eq!(ctx[0].role, Role::System);
        let text = ctx[0].text_content().unwrap();
        let p_prompt = text.find("You are helpful").expect("prompt in merged system message");
        let p_soul = text
            .find("[Core memory — permanent preferences, identity, and facts]\n- My name is TestBot")
            .expect("soul block in merged system message");
        let p_know = text
            .find("[Knowledge Index]\n[P1] note — stuff")
            .expect("knowledge block in merged system message");
        assert!(p_prompt < p_soul && p_soul < p_know, "order: prompt, soul, knowledge");
        assert_eq!(ctx[1].role, Role::User);
        assert_eq!(ctx[1].text_content(), Some("hello"));
    }

    #[test]
    fn test_build_context_absorbs_leading_summary_message() {
        // Gitea #77: the conversation summary stored at history[0] by
        // summarize_room must be absorbed into the single leading system
        // message, not emitted as a separate system message.
        let mut mgr = MemoryManager::new(2000, 60, 30_000_000);
        let room = mgr.get_or_create("room1", "general", "", false);
        for i in 0..6 {
            room.history.append(make_msg(Role::User, &format!("msg {}", i)));
        }
        mgr.summarize_room(
            "room1",
            4,
            ChatMessage::system("[Conversation Summary — earlier messages compressed]\nstuff happened"),
        );

        let ctx = mgr.build_context("room1", "You are helpful", None, None);
        let system_count = ctx.iter().filter(|m| m.role == Role::System).count();
        assert_eq!(system_count, 1, "summary must merge into the single leading system message");
        let text = ctx[0].text_content().unwrap();
        assert!(text.starts_with("You are helpful"));
        assert!(text.contains("[Conversation Summary — earlier messages compressed]\nstuff happened"));
        // History continues with the first retained (non-system) message.
        assert_eq!(ctx[1].role, Role::User);
        assert_eq!(ctx[1].text_content(), Some("msg 4"));
        assert_eq!(ctx.len(), 3, "1 merged system + 2 retained history messages");
    }

    #[test]
    fn test_build_context_empty_soul_and_knowledge_stay_single_system() {
        let mut mgr = MemoryManager::new(2000, 60, 30_000_000);
        let room = mgr.get_or_create("room1", "general", "", false);
        room.history.append(make_msg(Role::User, "hi"));
        mgr.set_knowledge("room1", String::new());

        let ctx = mgr.build_context("room1", "prompt", None, None);
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx[0].role, Role::System);
        assert_eq!(ctx[0].text_content(), Some("prompt"));
    }

    #[test]
    fn test_room_state_new() {
        let state = RoomState::new("rid1", "general", "", false);
        assert_eq!(state.room_id, "rid1");
        assert_eq!(state.room_name, "general");
        assert_eq!(state.room_fname, "");
        assert!(!state.is_dm);
        assert_eq!(state.history.room_id, "rid1");
    }

    #[test]
    fn test_room_state_dm() {
        let state = RoomState::new("dm-uuid-123", "saru", "", true);
        assert_eq!(state.room_id, "dm-uuid-123");
        assert_eq!(state.room_name, "saru");
        assert_eq!(state.room_fname, "");
        assert!(state.is_dm);
    }

    #[test]
    fn test_room_state_fname_chinese() {
        let state = RoomState::new("rid-ch", "sen1-lin2-sheng1-tai4", "森林生態", false);
        assert_eq!(state.room_id, "rid-ch");
        assert_eq!(state.room_name, "sen1-lin2-sheng1-tai4");
        assert_eq!(state.room_fname, "森林生態");
        assert!(!state.is_dm);
    }

    #[test]
    fn test_build_context_with_dm_name() {
        let mut mgr = MemoryManager::new(2000, 60, 30_000_000);
        let room = mgr.get_or_create("dm-xyz", "alice", "", true);
        assert_eq!(room.room_name, "alice");
        assert!(room.is_dm);
        room.history.append(ChatMessage::user("alice: hello"));
        let ctx = mgr.build_context("dm-xyz", "prompt", None, None);
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx[1].text_content(), Some("alice: hello"));
    }

    #[test]
    fn test_strip_orphaned_tool_calls_trailing() {
        let tc = crate::types::ToolCall::new("c1", "search", r#"{"q":"x"}"#);
        let mut msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hi"),
            ChatMessage::assistant_with_tool_calls("", vec![tc.clone()], None),
        ];
        strip_orphaned_tool_calls(&mut msgs);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].role, Role::User);
    }

    #[test]
    fn test_strip_orphaned_tool_calls_with_reply_preserved() {
        let tc = crate::types::ToolCall::new("c1", "search", r#"{"q":"x"}"#);
        let mut msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hi"),
            ChatMessage::assistant_with_tool_calls("", vec![tc.clone()], None),
            ChatMessage::tool("c1", "result"),
            ChatMessage::assistant("final reply"),
        ];
        let len_before = msgs.len();
        strip_orphaned_tool_calls(&mut msgs);
        assert_eq!(msgs.len(), len_before);
        assert_eq!(msgs[2].role, Role::Assistant);
        assert_eq!(msgs[3].role, Role::Tool);
    }

    #[test]
    fn test_strip_orphaned_tool_calls_no_op() {
        let mut msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("reply"),
        ];
        strip_orphaned_tool_calls(&mut msgs);
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn test_strip_orphaned_tool_messages_at_start() {
        let mut msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::tool("c1", "result"),
            ChatMessage::user("next message"),
        ];
        strip_orphaned_tool_calls(&mut msgs);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::User);
    }

    #[test]
    fn test_strip_orphaned_tool_messages_mid_list() {
        let mut msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("reply"),
            ChatMessage::tool("orphan", "result"),
            ChatMessage::user("next"),
        ];
        strip_orphaned_tool_calls(&mut msgs);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[3].role, Role::User);
    }

    #[test]
    fn test_strip_orphaned_tool_messages_with_valid_pair_preserved() {
        let tc = crate::types::ToolCall::new("c1", "search", r#"{"q":"x"}"#);
        let mut msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::tool("orphan_start", "oops"),
            ChatMessage::user("hi"),
            ChatMessage::assistant_with_tool_calls("", vec![tc], None),
            ChatMessage::tool("c1", "result"),
            ChatMessage::assistant("final"),
        ];
        strip_orphaned_tool_calls(&mut msgs);
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::User);
        assert_eq!(msgs[2].role, Role::Assistant);
        assert_eq!(msgs[3].role, Role::Tool);
        assert_eq!(msgs[4].role, Role::Assistant);
    }

    #[test]
    fn test_extract_identity_name_standard() {
        let content = "# Soul Memory\n\n- My name is 零夢\n- loves coffee";
        assert_eq!(extract_identity_name(content), Some("零夢".into()));
    }

    #[test]
    fn test_extract_identity_name_with_emoji() {
        let content = "# Soul Memory\n\n- My name is 零夢 ✨\n- likes cats";
        assert_eq!(extract_identity_name(content), Some("零夢 ✨".into()));
    }

    #[test]
    fn test_extract_identity_name_cjk() {
        let content = "# Soul Memory\n\n- My name is 雪山泡芙 ✨\n- prefers tea\n- dislikes spam";
        assert_eq!(extract_identity_name(content), Some("雪山泡芙 ✨".into()));
    }

    #[test]
    fn test_extract_identity_name_multiline() {
        let content = "# Soul Memory\n\n- My name is 零夢 ✨ test\n- more items";
        assert_eq!(extract_identity_name(content), Some("零夢 ✨ test".into()));
    }

    #[test]
    fn test_extract_identity_name_no_match() {
        let content = "# Soul Memory\n\n- likes cats\n- born 2024";
        assert_eq!(extract_identity_name(content), None);
    }

    #[test]
    fn test_extract_identity_name_too_long() {
        let content = "# Soul Memory\n\n- My name is A very very long name over 32 characters\n- other";
        assert_eq!(extract_identity_name(content), None);
    }

    #[test]
    fn test_extract_identity_name_not_matching() {
        let content = "Just some random text\nwith no name prefix at all";
        assert_eq!(extract_identity_name(content), None);
    }

    #[test]
    fn test_extract_identity_name_empty_content() {
        assert_eq!(extract_identity_name(""), None);
    }

    #[test]
    fn test_self_display_name_regex_match() {
        let mut mm = MemoryManager::new(2000, 60, 30_000_000);
        let soul = SoulMemory::from_raw(
            NonEmptyString::try_new("r-test".to_string()).unwrap(),
            "# Soul Memory\n\n- My name is 香菜 🌿\n- foo".into(),
            String::new(),
        );
        mm.set_soul("r-test", soul);
        assert_eq!(mm.self_display_name("r-test"), Some("香菜 🌿".into()));
    }

    #[test]
    fn test_self_display_name_no_myname() {
        let mut mm = MemoryManager::new(2000, 60, 30_000_000);
        let soul = SoulMemory::from_raw(
            NonEmptyString::try_new("r-test".to_string()).unwrap(),
            "零夢\n\n- foo".into(),
            String::new(),
        );
        mm.set_soul("r-test", soul);
        // No "My name is" pattern — returns None
        assert_eq!(mm.self_display_name("r-test"), None);
    }

    #[test]
    fn test_truncate_message_content_text_under_limit() {
        let mut msg = ChatMessage::user("Hello world");
        truncate_message_content(&mut msg, 100);
        assert_eq!(msg.text_content(), Some("Hello world"));
    }

    #[test]
    fn test_truncate_message_content_text_over_limit() {
        let mut msg = ChatMessage::user("a".repeat(300_000));
        truncate_message_content(&mut msg, 200_000);
        let content = msg.text_content().unwrap();
        assert!(content.len() < 300_000);
        assert!(content.contains("[...truncated]"));
        assert!(content.starts_with(&"a".repeat(1000)));
    }

    #[test]
    fn test_truncate_message_content_multipart_text_under_limit() {
        let mut msg = crate::types::ChatMessage {
            role: crate::types::Role::User,
            content: crate::types::MessageContent::Multipart(vec![
                crate::types::ContentPart::Text { text: "short text".into() },
                crate::types::ContentPart::ImageUrl {
                    image_url: crate::types::ImageUrlPayload {
                        url: "http://example.com/img.png".into(),
                        detail: None,
                    },
                },
            ]),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        };
        truncate_message_content(&mut msg, 100);
        // Should be unchanged
        match &msg.content {
            crate::types::MessageContent::Multipart(parts) => {
                assert_eq!(parts.len(), 2);
                if let crate::types::ContentPart::Text { text } = &parts[0] {
                    assert_eq!(text, "short text");
                } else {
                    panic!("expected text part");
                }
            }
            _ => panic!("expected multipart"),
        }
    }

    #[test]
    fn test_truncate_message_content_multipart_text_over_limit() {
        let mut msg = crate::types::ChatMessage {
            role: crate::types::Role::User,
            content: crate::types::MessageContent::Multipart(vec![
                crate::types::ContentPart::Text { text: "b".repeat(300_000) },
                crate::types::ContentPart::ImageUrl {
                    image_url: crate::types::ImageUrlPayload {
                        url: "http://example.com/img.png".into(),
                        detail: None,
                    },
                },
            ]),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        };
        truncate_message_content(&mut msg, 200_000);
        match &msg.content {
            crate::types::MessageContent::Multipart(parts) => {
                assert_eq!(parts.len(), 2);
                // Image part preserved
                assert!(matches!(parts[1], crate::types::ContentPart::ImageUrl { .. }));
                // Text part truncated
                if let crate::types::ContentPart::Text { text } = &parts[0] {
                    assert!(text.len() < 300_000);
                    assert!(text.contains("[...truncated]"));
                } else {
                    panic!("expected text part");
                }
            }
            _ => panic!("expected multipart"),
        }
    }

    #[test]
    fn test_truncate_message_content_preserves_role() {
        let mut msg = ChatMessage::assistant("x".repeat(300_000));
        assert_eq!(msg.role, crate::types::Role::Assistant);
        truncate_message_content(&mut msg, 100_000);
        assert_eq!(msg.role, crate::types::Role::Assistant);
    }

    #[test]
    fn test_count_json_bytes_increasing_with_content() {
        let short = ChatMessage::user("hi");
        let long = ChatMessage::user("x".repeat(1000));
        let short_bytes = count_json_bytes(&short);
        let long_bytes = count_json_bytes(&long);
        assert!(long_bytes > short_bytes, "longer content should produce more bytes: {short_bytes} vs {long_bytes}");
    }

    #[test]
    fn test_count_json_bytes_produces_same_result_as_to_string() {
        let msg = ChatMessage::user("hello world");
        let via_string = serde_json::to_string(&msg).unwrap().len();
        let via_counter = count_json_bytes(&msg);
        assert_eq!(via_counter, via_string, "count_json_bytes should match serde_json::to_string().len()");
    }

    #[test]
    fn test_count_json_bytes_empty_message() {
        let msg = ChatMessage::user("");
        assert!(count_json_bytes(&msg) > 0, "even empty message has JSON structure bytes");
    }

    // ─── Soul front matter / migration ──────────────────────────────────────

    fn soul(raw: &str) -> SoulMemory {
        SoulMemory::from_raw(
            NonEmptyString::try_new("r-test".to_string()).unwrap(),
            raw.to_string(),
            String::new(),
        )
    }

    #[test]
    fn test_soul_parse_legacy_has_no_front_matter() {
        let doc = SoulDocument::parse("# Soul Memory\n\n- My name is 零夢 ✨\n");
        assert!(!doc.had_front_matter);
        assert!(doc.broken_front_matter.is_none());
        assert_eq!(doc.body, "# Soul Memory\n\n- My name is 零夢 ✨\n");
        assert!(doc.front.is_empty());
    }

    #[test]
    fn test_soul_parse_front_matter() {
        let raw = "---\nname: 零夢 ✨\npronouns: she/her\naddress_user_as: Master 🐵\nlanguage: en\nschema: 2\nupdated: 2026-11-05\n---\n\n# Soul Memory\n\n- likes tea\n";
        let doc = SoulDocument::parse(raw);
        assert!(doc.had_front_matter);
        assert!(doc.broken_front_matter.is_none());
        assert_eq!(doc.front.name.as_ref().map(SoulName::as_str), Some("零夢 ✨"));
        assert_eq!(doc.front.pronouns.as_deref(), Some("she/her"));
        assert_eq!(doc.front.address_user_as.as_deref(), Some("Master 🐵"));
        assert_eq!(doc.front.language.as_deref(), Some("en"));
        assert_eq!(doc.front.schema, Some(2));
        assert_eq!(doc.front.updated.as_deref(), Some("2026-11-05"));
        assert_eq!(doc.body, "# Soul Memory\n\n- likes tea\n");
    }

    #[test]
    fn test_soul_parse_crlf_and_bom() {
        let raw = "\u{feff}---\r\nname: Zero\r\n---\r\n\r\n- likes tea\r\n";
        let doc = SoulDocument::parse(raw);
        assert!(doc.had_front_matter);
        assert_eq!(doc.front.name.as_ref().map(SoulName::as_str), Some("Zero"));
        assert_eq!(doc.body, "- likes tea\n");
    }

    #[test]
    fn test_soul_parse_quoted_values() {
        let doc = SoulDocument::parse("---\nname: \"零夢 ✨\"\nlanguage: 'zh-HK'\n---\n\n- foo");
        assert_eq!(doc.front.name.as_ref().map(SoulName::as_str), Some("零夢 ✨"));
        assert_eq!(doc.front.language.as_deref(), Some("zh-HK"));
    }

    #[test]
    fn test_soul_parse_unknown_keys_preserved_on_render() {
        let raw = "---\nname: Zero\npersona_version: 3\n---\n\n- foo";
        let doc = SoulDocument::parse(raw);
        assert_eq!(doc.front.extra, vec![("persona_version".to_string(), "3".to_string())]);
        let rendered = doc.render();
        assert!(rendered.contains("persona_version: 3"), "rendered: {rendered}");
    }

    #[test]
    fn test_soul_parse_unterminated_block_is_body() {
        let raw = "---\nname: Zero\n\n- foo";
        let doc = SoulDocument::parse(raw);
        assert!(!doc.had_front_matter);
        assert!(doc.broken_front_matter.is_none());
        assert_eq!(doc.body, raw);
    }

    #[test]
    fn test_soul_parse_horizontal_rule_is_body() {
        let raw = "---\n\n- item\n";
        let doc = SoulDocument::parse(raw);
        assert!(!doc.had_front_matter);
        assert_eq!(doc.body, raw);
    }

    #[test]
    fn test_soul_parse_broken_block_sets_flag_and_keeps_raw() {
        let raw = "---\nnot a pair\n---\n\n- item";
        let doc = SoulDocument::parse(raw);
        assert!(!doc.had_front_matter);
        assert!(doc.broken_front_matter.is_some());
        assert_eq!(doc.body, raw);
    }

    #[test]
    fn test_soul_render_round_trip() {
        let raw = "---\nname: 零夢 ✨\nlanguage: en\nschema: 2\nupdated: 2026-11-05\n---\n\n# Soul Memory\n\n- likes tea";
        let first = SoulDocument::parse(raw);
        let second = SoulDocument::parse(&first.render());
        assert_eq!(first, second);
    }

    #[test]
    fn test_soul_needs_migration_only_for_legacy_with_name() {
        assert!(soul("# Soul Memory\n\n- My name is 零夢 ✨").needs_migration());
        assert!(!soul("---\nname: 零夢\n---\n\n- foo").needs_migration());
        assert!(!soul("# Soul Memory\n\n- likes tea").needs_migration());
        assert!(!soul("---\nnot a pair\n---\n- My name is Zero").needs_migration());
    }

    #[test]
    fn test_soul_migrated_moves_bullet_into_front_name() {
        let doc = soul("# Soul Memory\n\n- My name is 零夢 ✨\n- I like tea").migrated();
        assert!(doc.had_front_matter);
        assert_eq!(doc.front.name.as_ref().map(SoulName::as_str), Some("零夢 ✨"));
        assert_eq!(doc.front.schema, Some(SOUL_SCHEMA_VERSION));
        assert_eq!(doc.front.updated.as_deref(), Some(today_iso_date().as_str()));
        assert!(!doc.body.contains("My name is"), "body: {}", doc.body);
        assert!(doc.body.contains("- I like tea"));
    }

    #[test]
    fn test_soul_migrated_removes_identity_bullet_even_with_prose() {
        let doc = soul("- My name is Zero ✨ and I like tea\n- title: keep me").migrated();
        // The whole tail of the bullet becomes the name, so nothing is lost.
        assert_eq!(
            doc.front.name.as_ref().map(SoulName::as_str),
            Some("Zero ✨ and I like tea")
        );
        assert!(!doc.body.contains("My name is"));
        assert!(doc.body.contains("- title: keep me"));
    }

    #[test]
    fn test_soul_migrated_is_idempotent() {
        let doc = soul("- My name is Zero ✨\n- other").migrated();
        let migrated = soul(&doc.render());
        assert!(!migrated.needs_migration());
        assert_eq!(migrated.front.name, doc.front.name);
        assert_eq!(migrated.body.trim(), "- other");
    }

    #[test]
    fn test_strip_identity_bullet_handles_star_and_trailing_space() {
        assert_eq!(
            strip_identity_bullet("* My name is Zero ✨  \n- other", "Zero ✨"),
            "- other"
        );
        assert_eq!(
            strip_identity_bullet("- Keep me\n- My name is Zero\n- Keep too\n", "Zero"),
            "- Keep me\n- Keep too\n"
        );
    }

    #[test]
    fn test_extract_identity_name_counts_chars_not_bytes() {
        let name = "零".repeat(20); // 20 chars, 60 bytes
        let content = format!("# Soul Memory\n\n- My name is {name}");
        assert_eq!(extract_identity_name(&content), Some(name));
    }

    #[test]
    fn test_soul_name_rejects_multiline_and_long() {
        assert!(SoulName::try_new("").is_err());
        assert!(SoulName::try_new("a\nb").is_err());
        assert!(SoulName::try_new(&"x".repeat(33)).is_err());
        assert!(SoulName::try_new(&"零".repeat(32)).is_ok());
    }

    #[test]
    fn test_build_context_strips_front_matter() {
        let mut mm = MemoryManager::new(2000, 60, 30_000_000);
        let doc = soul("---\nname: 零夢 ✨\nlanguage: en\n---\n\n# Soul Memory\n\n- likes tea");
        mm.set_soul("r-test", doc);
        let ctx = mm.build_context("r-test", "prompt", None, None);
        let text = ctx[0].text_content().unwrap();
        assert!(text.contains("[Identity — name: 零夢 ✨, language: en]"), "text: {text}");
        assert!(text.contains("# Soul Memory"), "text: {text}");
        assert!(!text.contains("---"), "front matter must be stripped: {text}");
    }

    #[test]
    fn test_build_context_legacy_prompt_unchanged() {
        let mut mm = MemoryManager::new(2000, 60, 30_000_000);
        mm.set_soul("r-test", soul("# Soul Memory\n\n- My name is 零夢 ✨\n- likes tea"));
        let ctx = mm.build_context("r-test", "prompt", None, None);
        let text = ctx[0].text_content().unwrap();
        assert!(text.contains("- My name is 零夢 ✨"), "text: {text}");
        assert!(!text.contains("[Identity"), "legacy souls keep their prompt: {text}");
    }

    #[test]
    fn test_build_context_truncates_body_not_front_matter() {
        let mut mm = MemoryManager::new(10, 60, 30_000_000);
        let body = format!("---\nname: {}", "Zero".repeat(8))
            + "\n---\n\n- 0123456789ABCDEFGHIJ";
        mm.set_soul("r-test", soul(&body));
        let ctx = mm.build_context("r-test", "prompt", None, None);
        let text = ctx[0].text_content().unwrap();
        // Identity line (derived from front matter) is not part of the budget.
        assert!(text.contains(&format!("name: {}", "Zero".repeat(8))), "text: {text}");
        assert!(text.contains("[truncated]"), "text: {text}");
    }

    #[test]
    fn test_identity_line_deduped_when_body_keeps_bullet() {
        let mut mm = MemoryManager::new(2000, 60, 30_000_000);
        let raw = "---\nname: 零夢 ✨\n---\n\n- My name is 零夢 ✨ and I like tea";
        mm.set_soul("r-test", soul(raw));
        let ctx = mm.build_context("r-test", "prompt", None, None);
        let text = ctx[0].text_content().unwrap();
        assert!(!text.contains("[Identity"), "no duplicate identity line: {text}");
        assert!(text.contains("My name is 零夢 ✨ and I like tea"));
    }

    #[test]
    fn test_self_display_name_prefers_front_matter() {
        let mm_soul = soul("---\nname: FrontName\n---\n\n- My name is BodyName");
        assert_eq!(mm_soul.front.name.as_ref().map(SoulName::as_str), Some("FrontName"));
        let mut mm = MemoryManager::new(2000, 60, 30_000_000);
        mm.set_soul("r-test", mm_soul);
        assert_eq!(mm.self_display_name("r-test"), Some("FrontName".into()));
        assert_eq!(mm.soul_name("r-test"), Some("FrontName"));
    }

    #[test]
    fn test_soul_normalized_for_write_uses_previous_name() {
        let doc = SoulDocument::parse("- no name here\n- other")
            .normalized_for_write(Some(&SoulName::try_new("OldName").unwrap()));
        assert_eq!(doc.front.name.as_ref().map(SoulName::as_str), Some("OldName"));
        assert_eq!(doc.front.schema, Some(SOUL_SCHEMA_VERSION));
        assert!(doc.render().contains("name: OldName"));
    }

    #[test]
    fn test_claim_soul_migration_once() {
        let mut mm = MemoryManager::new(2000, 60, 30_000_000);
        assert!(mm.claim_soul_migration("r-test"));
        assert!(!mm.claim_soul_migration("r-test"));
        assert!(mm.claim_soul_migration("r-other"));
    }
}
