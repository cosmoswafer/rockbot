use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use webdav::WebDavPath;

use crate::types::ChatMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryJson {
    pub schema: String,
    pub seq: u64,
    pub room_id: String,
    pub summary: String,
    pub date_range: String,
    pub msg_count: usize,
    pub messages: Vec<MessageRef>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySummary {
    pub date: String,
    pub summary: String,
    pub msg_count: usize,
    pub char_count: usize,
}

#[derive(Debug, Clone)]
pub struct SoulMemory {
    pub room_id: String,
    pub content: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRef {
    pub id: String,
    pub author: String,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub seq: u64,
    pub summary: String,
    pub date_range: String,
    pub msg_count: usize,
}

impl ArchiveEntry {
    pub fn new(seq: u64, summary: String, date_range: String, msg_count: usize) -> Self {
        Self {
            seq,
            summary,
            date_range,
            msg_count,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConversationHistory {
    pub room_id: String,
    pub messages: Vec<ChatMessage>,
    pub char_count: usize,
    pub archive_seq: u64,
    pub restored_summary: Option<String>,
}

impl ConversationHistory {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            messages: Vec::new(),
            char_count: 0,
            archive_seq: 0,
            restored_summary: None,
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

    pub fn set_archive_seq(&mut self, seq: u64) {
        self.archive_seq = seq;
    }
}

#[derive(Debug, Clone)]
pub struct RoomState {
    pub room_id: String,
    pub room_name: String,
    pub room_fname: String,
    pub is_dm: bool,
    pub history: ConversationHistory,
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
        }
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
    daily_summaries: HashMap<String, Vec<DailySummary>>,
    souls: HashMap<String, SoulMemory>,
    knowledge: HashMap<String, String>,
}

impl MemoryManager {
    pub fn new(
        max_chars: usize,
        max_history_messages: usize,
        max_summary_chars: usize,
        summary_days: u32,
        max_soul_chars: usize,
    ) -> Self {
        Self {
            rooms: HashMap::new(),
            max_chars,
            max_history_messages,
            max_summary_chars,
            summary_days,
            max_soul_chars,
            daily_summaries: HashMap::new(),
            souls: HashMap::new(),
            knowledge: HashMap::new(),
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

    pub fn check_and_archive(&mut self, room_id: &str) -> Option<(String, Vec<ChatMessage>, u64)> {
        let room = self.rooms.get(room_id)?;
        if room.history.needs_archive(self.max_chars) {
            let to_archive = room
                .history
                .oldest_messages(room.history.messages.len() / 2);
            let messages: Vec<ChatMessage> = to_archive.to_vec();
            let room_id = room.room_id.clone();
            let seq = room.history.archive_seq;
            Some((room_id, messages, seq))
        } else {
            None
        }
    }

    pub fn restore_from_archives(
        &mut self,
        room_id: &str,
        room_name: &str,
        room_fname: &str,
        is_dm: bool,
        archives: &[MemoryJson],
    ) {
        if archives.is_empty() {
            return;
        }

        let room = self.get_or_create(room_id, room_name, room_fname, is_dm);

        let max_seq = archives.iter().map(|a| a.seq).max().unwrap_or(0);
        room.history.set_archive_seq(max_seq + 1);

        let latest = archives.last().unwrap();
        let mut summary = format!(
            "[Restored conversation context from {} ({} messages archived across {} segments)]\n{}",
            latest.date_range,
            archives.iter().map(|a| a.msg_count).sum::<usize>(),
            archives.len(),
            latest.summary,
        );

        if archives.len() > 1 {
            for a in archives.iter().rev().skip(1) {
                summary.push_str(&format!("\nEarlier summary (#{}): {}", a.seq, a.summary));
            }
        }

        room.history.restored_summary = Some(summary);
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

        if let Some(soul) = self.souls.get(room_id) {
            if !soul.content.is_empty() {
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
                    "[Core memory — permanent preferences, identity, and notes]\n{}",
                    truncated
                )));
            }
        }

        if let Some(knowledge_text) = self.knowledge.get(room_id) {
            if !knowledge_text.is_empty() {
                messages.push(ChatMessage::system(knowledge_text));
            }
        }

        let summaries = self.daily_summaries.get(room_id).map(|v| v.as_slice()).unwrap_or(&[]);
        if !summaries.is_empty() {
            let mut summary_text = String::from("[Recent conversation summaries]\n");
            let mut total = 0usize;
            for s in summaries.iter().rev() {
                let line = format!("## {} ({} messages)\n{}\n\n", s.date, s.msg_count, s.summary);
                if total + line.len() > self.max_summary_chars {
                    break;
                }
                total += line.len();
                summary_text.push_str(&line);
            }
            messages.push(ChatMessage::system(&summary_text));
        }

        if let Some(room) = self.rooms.get(room_id) {
            if let Some(ref restored) = room.history.restored_summary {
                messages.push(ChatMessage::system(restored.as_str()));
            }
            let history = &room.history.messages;
            let start = if history.len() > limit {
                history.len() - limit
            } else {
                0
            };
            messages.extend_from_slice(&history[start..]);
        }

        if let Some(extra) = extra_context {
            messages.splice(1..1, extra);
        }

        strip_orphaned_tool_calls(&mut messages);

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

    pub fn set_knowledge(&mut self, room_id: &str, entries: String) {
        self.knowledge.insert(room_id.to_string(), entries);
    }

    pub fn get_knowledge(&self, room_id: &str) -> Option<&str> {
        self.knowledge.get(room_id).map(|s| s.as_str())
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
    if m < 1 || m > 12 || d < 1 || d > 31 {
        return None;
    }
    let m = if m <= 2 { m + 12 } else { m };
    let y = if m > 12 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (m as u64 - 3) + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era as i64) * 146097 + doe as i64 - 719468)
}

fn strip_orphaned_tool_calls(messages: &mut Vec<ChatMessage>) {
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

    // Phase 2: remove trailing assistant tool_calls that have no tool
    // replies (e.g. the AI's last message was a tool call that never
    // completed before the loop was interrupted).
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role == crate::types::Role::Assistant && messages[i].tool_calls.is_some() {
            let mut has_tool_reply = false;
            for item in messages.iter().skip(i + 1) {
                if item.role == crate::types::Role::Tool {
                    has_tool_reply = true;
                    break;
                }
                if item.role != crate::types::Role::Tool {
                    break;
                }
            }
            if !has_tool_reply && i + 1 >= messages.len() {
                messages.truncate(i);
                break;
            }
        }
        i += 1;
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
    fn test_archive_entry_new() {
        let entry = ArchiveEntry::new(0, "Summary".into(), "2025-01-01".into(), 10);
        assert_eq!(entry.seq, 0);
        assert_eq!(entry.summary, "Summary");
        assert_eq!(entry.date_range, "2025-01-01");
        assert_eq!(entry.msg_count, 10);
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
        let mut mgr = MemoryManager::new(1000, 12, 8000, 7, 2000);
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
        let mut mgr = MemoryManager::new(1000, 3, 8000, 7, 2000);
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
        let mgr = MemoryManager::new(1000, 12, 8000, 7, 2000);
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
        let mut mgr = MemoryManager::new(1000, 12, 8000, 7, 2000);
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
}
