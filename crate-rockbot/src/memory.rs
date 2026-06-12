use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::debug;
use webdav::WebDavPath;

use crate::types::ChatMessage;
use crate::utils::now_iso_string;
use crate::validated::NonEmptyString;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistSnapshot {
    pub schema: NonEmptyString,
    pub room_id: NonEmptyString,
    pub messages: Vec<ChatMessage>,
    pub char_count: usize,
    pub archive_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soul: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub daily_summaries: Vec<DailySummary>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySummary {
    pub date: NonEmptyString,
    pub summary: String,
    pub msg_count: usize,
    pub char_count: usize,
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
    max_chars: usize,
    max_history_messages: usize,
    pub max_summary_chars: usize,
    pub summary_days: u32,
    pub max_soul_chars: usize,
    max_context_bytes: usize,
    daily_summaries: HashMap<String, Vec<DailySummary>>,
    souls: HashMap<String, SoulMemory>,
    knowledge: HashMap<String, String>,
    dirty_snapshots: HashSet<String>,
    pub persist_interval_secs: u64,
}

impl MemoryManager {
    pub fn new(
        max_chars: usize,
        max_history_messages: usize,
        max_summary_chars: usize,
        summary_days: u32,
        max_soul_chars: usize,
        persist_interval_secs: u64,
        max_context_bytes: usize,
    ) -> Self {
        Self {
            rooms: HashMap::new(),
            max_chars,
            max_history_messages,
            max_summary_chars,
            summary_days,
            max_soul_chars,
            max_context_bytes,
            daily_summaries: HashMap::new(),
            souls: HashMap::new(),
            knowledge: HashMap::new(),
            dirty_snapshots: HashSet::new(),
            persist_interval_secs,
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

    pub fn check_and_archive(&mut self, room_id: &str) -> Option<(String, Vec<ChatMessage>)> {
        let room = self.rooms.get(room_id)?;
        if room.history.needs_archive(self.max_chars) {
            let to_archive = room
                .history
                .oldest_messages(room.history.messages.len() / 2);
            let messages: Vec<ChatMessage> = to_archive.to_vec();
            let room_id = room.room_id.clone();
            Some((room_id, messages))
        } else {
            None
        }
    }

    pub fn prune_archived(&mut self, room_id: &str, count: usize) {
        if let Some(room) = self.rooms.get_mut(room_id) {
            room.history.prune_first(count);
        }
    }

    pub fn build_context(
        &self,
        room_id: &str,
        system_prompt: &str,
        max_history: Option<usize>,
        extra_context: Option<Vec<ChatMessage>>,
    ) -> Vec<ChatMessage> {
        let limit = max_history.unwrap_or(self.max_history_messages);
        let mut messages = Vec::new();
        messages.push(ChatMessage::system(system_prompt));
        let mut has_soul = false;
        let mut has_knowledge = false;
        let mut summary_count = 0usize;

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
                messages.push(ChatMessage::system(format!(
                    "[Core memory — permanent preferences, identity, and facts]\n{}",
                    truncated
                )));
            }
        }

        if let Some(knowledge_text) = self.knowledge.get(room_id) {
            if !knowledge_text.is_empty() {
                has_knowledge = true;
                messages.push(ChatMessage::system(knowledge_text));
            }
        }

        let summaries = self.daily_summaries.get(room_id).map(|v| v.as_slice()).unwrap_or(&[]);
        if !summaries.is_empty() {
            summary_count = summaries.len();
            let mut summary_text = String::from("[Recent conversation summaries]\n");
            let mut total = 0usize;
            for s in summaries.iter().rev() {
                let line = format!("## {} ({} messages)\n{}\n\n", s.date.as_str(), s.msg_count, s.summary);
                if total + line.len() > self.max_summary_chars {
                    break;
                }
                total += line.len();
                summary_text.push_str(&line);
            }
            messages.push(ChatMessage::system(&summary_text));
        }

        if let Some(room) = self.rooms.get(room_id) {
            let history = &room.history.messages;
            let start = if history.len() > limit {
                history.len() - limit
            } else {
                0
            };
            let slice = &history[start..];
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
            "build_context room={}: total={} system_msgs={} history_msgs={} soul={} knowledge={} summaries={}",
            room_id, messages.len(),
            messages.iter().filter(|m| m.role == crate::types::Role::System).count(),
            history_count,
            has_soul, has_knowledge, summary_count,
        );

        // Enforce max_context_bytes: drop oldest images until under limit.
        if self.max_context_bytes > 0 {
            let mut total = messages
                .iter()
                .map(|m| serde_json::to_vec(m).map(|v| v.len()).unwrap_or(0))
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
                    let before = serde_json::to_vec(&messages[i]).map(|v| v.len()).unwrap_or(0);
                    messages[i] = strip_images_from_message(messages[i].clone());
                    let after = serde_json::to_vec(&messages[i]).map(|v| v.len()).unwrap_or(0);
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

    pub fn daily_summaries_dir(&self, webdav_dir: &str) -> String {
        format!("{}memory/summaries/", WebDavPath::new("").room_dir(webdav_dir))
    }

    pub fn set_daily_summaries(&mut self, room_id: &str, summaries: Vec<DailySummary>) {
        let recent: Vec<DailySummary> = summaries
            .into_iter()
            .filter(|s| self.is_summary_recent(&s.date))
            .take(10)
            .collect();
        self.daily_summaries.insert(room_id.to_string(), recent);
    }

    pub fn get_daily_summaries(&self, room_id: &str) -> &[DailySummary] {
        // Return empty slice if not loaded yet (graceful, not a panic)
        self.daily_summaries.get(room_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn is_summary_recent(&self, date: &str) -> bool {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let now_days = (now.as_secs() / 86400) as i64;
        let date_days = date_to_days(date);
        date_days.map(|d| now_days - d < self.summary_days as i64).unwrap_or(false)
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
            daily_summaries: Vec::new(),
            updated_at,
        };

        if let Some(soul) = self.souls.get(room_id) {
            if !soul.content.is_empty() {
                snapshot.soul = Some(soul.content.clone());
            }
        }

        if let Some(summaries) = self.daily_summaries.get(room_id) {
            snapshot.daily_summaries = summaries.clone();
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
            let room_key: String = snapshot.room_id.to_string();
            self.souls.insert(room_key, soul);
        }

        if !snapshot.daily_summaries.is_empty() {
            let room_key: String = snapshot.room_id.to_string();
            self.daily_summaries.insert(
                room_key,
                snapshot.daily_summaries.clone(),
            );
        }
    }

    pub fn room_ids(&self) -> Vec<String> {
        self.rooms.keys().cloned().collect()
    }

    pub fn evict_room(&mut self, room_id: &str) -> Option<RoomState> {
        self.daily_summaries.remove(room_id);
        self.souls.remove(room_id);
        self.knowledge.remove(room_id);
        self.dirty_snapshots.remove(room_id);
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

pub fn date_to_days(date: &str) -> Option<i64> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let m = if m <= 2 { m + 12 } else { m };
    let y = if m > 12 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (m as u64 - 3) + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe as i64 - 719468)
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
        let mut mgr = MemoryManager::new(1000, 12, 8000, 7, 2000, 60, 30_000_000);
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
        let mut mgr = MemoryManager::new(1000, 3, 8000, 7, 2000, 60, 30_000_000);
        let room = mgr.get_or_create("room1", "general", "", false);
        for i in 0..5 {
            room.history
                .append(make_msg(Role::User, &format!("msg {}", i)));
        }

        let ctx = mgr.build_context("room1", "You are helpful", None, None);
        assert_eq!(ctx.len(), 4);
        assert_eq!(ctx[0].role, Role::System);
        assert_eq!(ctx[1].text_content(), Some("msg 2"));
        assert_eq!(ctx[2].text_content(), Some("msg 3"));
        assert_eq!(ctx[3].text_content(), Some("msg 4"));
    }

    #[test]
    fn test_memory_manager_build_context_nonexistent_room() {
        let mgr = MemoryManager::new(1000, 12, 8000, 7, 2000, 60, 30_000_000);
        let ctx = mgr.build_context("nonexistent", "prompt", None, None);
        assert_eq!(ctx.len(), 1);
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
        let mut mgr = MemoryManager::new(1000, 12, 8000, 7, 2000, 60, 30_000_000);
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
        let mut mm = MemoryManager::new(1000, 12, 8000, 7, 2000, 60, 30_000_000);
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
        let mut mm = MemoryManager::new(1000, 12, 8000, 7, 2000, 60, 30_000_000);
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
}
