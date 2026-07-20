use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use tracing::debug;

use crate::types::ChatMessage;
use crate::utils::now_iso_string;
use crate::validated::NonEmptyString;

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct PersistSnapshot {
    pub schema: NonEmptyString,
    pub room_id: NonEmptyString,
    pub messages: Vec<ChatMessage>,
    pub char_count: usize,
    pub archive_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soul: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[validate(min_length = 1)]
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct SoulMemory {
    pub room_id: NonEmptyString,
    pub content: String,
    pub updated_at: String,
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
            if !soul.content.is_empty() {
                has_soul = true;
                let truncated = if soul.content.chars().count() > self.max_soul_chars {
                    let mut chars: Vec<char> = soul.content.chars().collect();
                    chars.truncate(self.max_soul_chars);
                    let mut t: String = chars.into_iter().collect();
                    t.push_str("\n\n[truncated]");
                    t
                } else {
                    soul.content.clone()
                };
                system_parts.push(format!(
                    "[Core memory — permanent preferences, identity, and facts]\n{}",
                    truncated
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

    pub fn self_display_name(&self, room_id: &str) -> Option<String> {
        let soul = match self.souls.get(room_id) {
            Some(s) => s,
            None => {
                debug!("self_display_name: no soul found for room={}", room_id);
                return None;
            }
        };
        let content = soul.content.trim();
        if content.is_empty() {
            debug!("self_display_name: empty soul content for room={}", room_id);
            return None;
        }

        // Standard regex: `My name is (.+)` — captures from first item of flat list
        if let Some(name) = extract_identity_name(content) {
            debug!("self_display_name: regex match, room={} name={:?}", room_id, name);
            return Some(name);
        }

        debug!("self_display_name: no display name found for room={}", room_id);
        None
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
            let soul = SoulMemory {
                room_id: snapshot.room_id.clone(),
                content: soul_content.clone(),
                updated_at: snapshot.updated_at.clone(),
            };
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
/// first item of the flat enumeration list (always "My name is ...").
fn extract_identity_name(content: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"My name is (.+)").expect("hardcoded regex")
    });
    let caps = RE.captures(content)?;
    let name = caps.get(1)?.as_str().trim().to_string();
    if !name.is_empty() && name.len() <= 32 {
        Some(name)
    } else {
        None
    }
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

        let soul = SoulMemory {
            room_id: NonEmptyString::try_new("room1".to_string()).unwrap(),
            content: "- My name is TestBot".into(),
            updated_at: String::new(),
        };
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
        let soul = SoulMemory {
            room_id: NonEmptyString::try_new("r-test".to_string()).unwrap(),
            content: "# Soul Memory\n\n- My name is 香菜 🌿\n- foo".into(),
            updated_at: String::new(),
        };
        mm.set_soul("r-test", soul);
        assert_eq!(mm.self_display_name("r-test"), Some("香菜 🌿".into()));
    }

    #[test]
    fn test_self_display_name_no_myname() {
        let mut mm = MemoryManager::new(2000, 60, 30_000_000);
        let soul = SoulMemory {
            room_id: NonEmptyString::try_new("r-test".to_string()).unwrap(),
            content: "零夢\n\n- foo".into(),
            updated_at: String::new(),
        };
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
}
