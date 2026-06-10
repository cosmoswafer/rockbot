use serde::{Deserialize, Serialize};

/// An incoming message from RocketChat, parsed from a DDP "changed" event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    /// The message ID.
    pub msg_id: Option<String>,
    /// The room ID where the message was sent.
    pub room_id: String,
    /// The room name (URL slug from DDP `roomName`). Empty string or "DIRECT_MESSAGES" for DMs.
    pub room_name: String,
    /// The room friendly name (from DDP `fname`). Empty string when not set or for DMs.
    pub room_fname: String,
    /// The username of the sender (not user ID).
    pub sender_name: String,
    /// The message text. For @mentions, the bot name is stripped.
    pub text: String,
    /// Whether this is a direct message.
    pub is_dm: bool,
    /// The message timestamp (Unix epoch milliseconds).
    pub timestamp: Option<i64>,
    /// The user ID of the sender.
    pub sender_id: String,
}

/// A reply to be sent back to a RocketChat room.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotReply {
    /// The room ID to send the message to.
    pub room_id: String,
    /// The message text.
    pub text: String,
    /// Optional thread message ID for threaded replies.
    pub thread_id: Option<String>,
}

impl BotReply {
    pub fn new(room_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            text: text.into(),
            thread_id: None,
        }
    }
}

/// Builder for filtering incoming messages.
pub struct MessageFilter<'a> {
    bot_user_id: &'a str,
}

impl<'a> MessageFilter<'a> {
    pub fn new(bot_user_id: &'a str) -> Self {
        Self { bot_user_id }
    }

    /// Returns Some(IncomingMessage) if the event should be dispatched,
    /// or None if it should be filtered out.
    ///
    /// When `room_cache` is provided, it is used to resolve `room_fname`
    /// if the per-event `args[1].fname` is absent or empty.
    pub fn filter(&self, raw: &serde_json::Value, room_cache: Option<&RoomCache>) -> Option<IncomingMessage> {
        let mut msg = Self::parse_message(raw)?;

        // Skip messages from the bot itself
        if msg.sender_id == self.bot_user_id {
            return None;
        }

        if msg.room_fname.is_empty() {
            if let Some(cache) = room_cache {
                if let Some(cached_fname) = cache.get_fname(&msg.room_id) {
                    msg.room_fname = cached_fname.to_string();
                }
            }
        }

        Some(msg)
    }

    fn parse_message(raw: &serde_json::Value) -> Option<IncomingMessage> {
        let fields = raw.get("fields")?;
        let args = fields.get("args")?.as_array()?;
        let first_arg = args.first()?;

        let msg_id = raw.get("id").and_then(|v| v.as_str()).map(String::from);

        let room_id = first_arg.get("rid")?.as_str()?.to_string();
        let text = first_arg
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let sender_id = first_arg
            .get("u")
            .and_then(|u| u.get("_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let sender_name = first_arg
            .get("u")
            .and_then(|u| u.get("username"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let timestamp = raw
            .get("fields")
            .and_then(|f| f.get("eventName"))
            .is_some()
            .then(|| {
                fields
                    .get("args")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|m| m.get("ts"))
                    .and_then(|ts| ts.get("$date").and_then(|d| d.as_i64()))
            })
            .flatten();

        let (room_name, is_dm) = if args.len() > 1 {
            let name = args[1]
                .get("roomName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let dm = name.is_empty() || name == "DIRECT_MESSAGES";
            (name, dm)
        } else {
            (String::new(), true)
        };

        let room_fname = if args.len() > 1 {
            args[1]
                .get("fname")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        Some(IncomingMessage {
            msg_id,
            room_id,
            room_name,
            room_fname,
            sender_name,
            text,
            is_dm,
            timestamp,
            sender_id,
        })
    }

    /// Check if a message is a DM or an @mention of the bot.
    pub fn is_dm_or_mention(
        msg: &IncomingMessage,
        bot_name: &str,
        registered_rooms: &std::collections::HashMap<String, bool>,
    ) -> bool {
        if msg.is_dm {
            return true;
        }
        if !msg.room_name.is_empty() && msg.text.starts_with(bot_name) {
            return true;
        }
        if !registered_rooms.is_empty()
            && !msg.room_name.is_empty()
            && registered_rooms.contains_key(&msg.room_name)
        {
            return true;
        }
        false
    }

    /// Strip the @botname prefix from message text.
    pub fn strip_mention(text: &str, bot_name: &str) -> String {
        let prefix = format!("{} ", bot_name);
        text.strip_prefix(&prefix)
            .or_else(|| text.strip_prefix(bot_name))
            .unwrap_or(text)
            .to_string()
    }

    /// Check if a room name is a registered room.
    pub fn is_registered_room(
        room_name: &str,
        registered_rooms: &std::collections::HashMap<String, bool>,
    ) -> bool {
        registered_rooms.contains_key(room_name)
    }
}

/// A cached room entry from the DDP "rooms" subscription.
#[derive(Debug, Clone)]
pub struct CachedRoom {
    pub room_id: String,
    pub name: String,
    pub fname: String,
    pub t: String,
}

/// In-memory cache of room metadata, keyed by RocketChat room UUID.
///
/// Populated at startup from the `"rooms"` DDP subscription and consulted
/// on every incoming message to resolve `room_fname` when the per-event
/// `args[1].fname` is absent or empty.
#[derive(Debug, Clone, Default)]
pub struct RoomCache {
    rooms: std::collections::HashMap<String, CachedRoom>,
}

impl RoomCache {
    pub fn new() -> Self {
        Self {
            rooms: std::collections::HashMap::new(),
        }
    }

    pub fn insert_from_added(&mut self, raw: &serde_json::Value) {
        let room_id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let fields = raw.get("fields");

        if let (Some(rid), Some(f)) = (room_id, fields) {
            let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let fname = f.get("fname").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let t = f.get("t").and_then(|v| v.as_str()).unwrap_or("").to_string();
            self.rooms.insert(
                rid.clone(),
                CachedRoom {
                    room_id: rid,
                    name,
                    fname,
                    t,
                },
            );
        }
    }

    pub fn get_fname(&self, room_id: &str) -> Option<&str> {
        self.rooms.get(room_id).and_then(|r| {
            if r.fname.is_empty() {
                None
            } else {
                Some(r.fname.as_str())
            }
        })
    }

    pub fn len(&self) -> usize {
        self.rooms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }
}
