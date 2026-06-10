use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    pub msg_id: Option<String>,
    pub room_id: String,
    pub room_name: String,
    pub room_fname: String,
    pub sender_name: String,
    pub text: String,
    pub is_dm: bool,
    pub timestamp: Option<i64>,
    pub sender_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotReply {
    pub room_id: String,
    pub text: String,
    pub thread_id: Option<String>,
}

impl BotReply {
    pub fn new(room_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self { room_id: room_id.into(), text: text.into(), thread_id: None }
    }
}

pub struct MessageFilter<'a> {
    bot_user_id: &'a str,
}

impl<'a> MessageFilter<'a> {
    pub fn new(bot_user_id: &'a str) -> Self {
        Self { bot_user_id }
    }

    pub fn filter(&self, raw: &serde_json::Value) -> Option<IncomingMessage> {
        let msg = Self::parse_message(raw)?;
        if msg.sender_id == self.bot_user_id {
            return None;
        }
        Some(msg)
    }

    fn parse_message(raw: &serde_json::Value) -> Option<IncomingMessage> {
        let fields = raw.get("fields")?;
        let args = fields.get("args")?.as_array()?;
        let first_arg = args.first()?;

        let msg_id = raw.get("id").and_then(|v| v.as_str()).map(String::from);
        let room_id = first_arg.get("rid")?.as_str()?.to_string();
        let text = first_arg.get("msg").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sender_id = first_arg.get("u")?.get("_id")?.as_str()?.to_string();
        let sender_name = first_arg.get("u")?.get("username")?.as_str()?.to_string();

        let timestamp = raw.get("fields")
            .and_then(|f| f.get("eventName")).is_some().then(|| {
                fields.get("args")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|m| m.get("ts"))
                    .and_then(|ts| ts.get("$date").and_then(|d| d.as_i64()))
            }).flatten();

        let (room_name, is_dm) = if args.len() > 1 {
            let name = args[1].get("roomName").and_then(|v| v.as_str()).unwrap_or("").to_string();
            (name.clone(), name.is_empty() || name == "DIRECT_MESSAGES")
        } else {
            (String::new(), true)
        };

        let room_fname = if args.len() > 1 {
            args[1].get("fname").and_then(|v| v.as_str()).unwrap_or("").to_string()
        } else {
            String::new()
        };

        Some(IncomingMessage {
            msg_id, room_id, room_name, room_fname, sender_name, text, is_dm, timestamp, sender_id,
        })
    }

    pub fn is_dm_or_mention(
        msg: &IncomingMessage, bot_name: &str, registered_rooms: &HashMap<String, bool>,
    ) -> bool {
        msg.is_dm || (!msg.room_name.is_empty() && msg.text.starts_with(bot_name))
            || (!registered_rooms.is_empty() && registered_rooms.contains_key(&msg.room_name))
    }

    pub fn strip_mention(text: &str, bot_name: &str) -> String {
        let prefix = format!("{} ", bot_name);
        text.strip_prefix(&prefix)
            .or_else(|| text.strip_prefix(bot_name))
            .unwrap_or(text).to_string()
    }

    pub fn is_registered_room(room_name: &str, registered_rooms: &HashMap<String, bool>) -> bool {
        registered_rooms.contains_key(room_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_filter_keeps_per_event_fname() {
        let event = serde_json::json!({
            "msg": "changed", "fields": {
                "eventName": "room1",
                "args": [
                    {"_id": "m1", "rid": "r1", "msg": "hello", "u": {"_id": "u1", "username": "user1"}, "ts": {"$date": 1000i64}},
                    {"roomName": "general", "fname": "Per-Event Fname"}
                ]
            }
        });
        let msg = MessageFilter::new("bot").filter(&event).unwrap();
        assert_eq!(msg.room_fname, "Per-Event Fname");
    }

    #[test]
    fn test_message_filter_keeps_empty_fname() {
        let event = serde_json::json!({
            "msg": "changed", "fields": {
                "eventName": "room1",
                "args": [
                    {"_id": "m1", "rid": "r1", "msg": "hello", "u": {"_id": "u1", "username": "user1"}, "ts": {"$date": 1000i64}},
                    {"roomName": "bare"}
                ]
            }
        });
        let msg = MessageFilter::new("bot").filter(&event).unwrap();
        assert_eq!(msg.room_fname, "");
    }
}
