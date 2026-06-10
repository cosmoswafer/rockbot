use std::collections::HashMap;

use crate::types::ChatMessage;

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
    pub is_dm: bool,
    pub history: ConversationHistory,
}

impl RoomState {
    pub fn new(room_id: impl Into<String>, room_name: impl Into<String>, is_dm: bool) -> Self {
        let room_id = room_id.into();
        Self {
            history: ConversationHistory::new(&room_id),
            room_id,
            room_name: room_name.into(),
            is_dm,
        }
    }
}

#[derive(Debug, Default)]
pub struct MemoryManager {
    rooms: HashMap<String, RoomState>,
    max_chars: usize,
    max_history_messages: usize,
}

impl MemoryManager {
    pub fn new(max_chars: usize, max_history_messages: usize) -> Self {
        Self {
            rooms: HashMap::new(),
            max_chars,
            max_history_messages,
        }
    }

    pub fn get_or_create(&mut self, room_id: &str, room_name: &str, is_dm: bool) -> &mut RoomState {
        self.rooms
            .entry(room_id.to_string())
            .or_insert_with(|| RoomState::new(room_id.to_string(), room_name.to_string(), is_dm))
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

        if let Some(room) = self.rooms.get(room_id) {
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

        messages
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
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
        let mut mgr = MemoryManager::new(1000, 12);
        let room = mgr.get_or_create("room1", "general", false);
        assert_eq!(room.room_id, "room1");
        assert_eq!(room.room_name, "general");
        assert!(!room.is_dm);

        let room2 = mgr.get_or_create("room1", "other", true);
        assert_eq!(room2.room_name, "general");
    }

    #[test]
    fn test_memory_manager_build_context() {
        let mut mgr = MemoryManager::new(1000, 3);
        let room = mgr.get_or_create("room1", "general", false);
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
        let mgr = MemoryManager::new(1000, 12);
        let ctx = mgr.build_context("nonexistent", "prompt", None, None);
        assert_eq!(ctx.len(), 1);
    }

    #[test]
    fn test_room_state_new() {
        let state = RoomState::new("rid1", "general", false);
        assert_eq!(state.room_id, "rid1");
        assert_eq!(state.room_name, "general");
        assert!(!state.is_dm);
        assert_eq!(state.history.room_id, "rid1");
    }
}
