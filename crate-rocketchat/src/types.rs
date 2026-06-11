use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub mime_type: String,
    pub size: u64,
    pub format: Option<String>,
    #[serde(rename = "typeGroup")]
    pub type_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDim {
    pub width: u64,
    pub height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub title: Option<String>,
    #[serde(rename = "title_link")]
    pub title_link: Option<String>,
    #[serde(rename = "title_link_download")]
    pub title_link_download: Option<bool>,
    #[serde(rename = "image_url")]
    pub image_url: Option<String>,
    #[serde(rename = "image_type")]
    pub image_type: Option<String>,
    #[serde(rename = "image_size")]
    pub image_size: Option<u64>,
    #[serde(rename = "image_dimensions")]
    pub image_dimensions: Option<ImageDim>,
    #[serde(rename = "image_preview")]
    pub image_preview: Option<String>,
    #[serde(rename = "type")]
    pub attach_type: Option<String>,
    #[serde(rename = "fileId")]
    pub file_id: Option<String>,
}

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
    pub alias: Option<String>,
    pub file: Option<FileInfo>,
    pub files: Vec<FileInfo>,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotReply {
    pub room_id: String,
    pub text: String,
    pub alias: Option<String>,
    pub thread_id: Option<String>,
}

impl BotReply {
    pub fn new(room_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self { room_id: room_id.into(), text: text.into(), alias: None, thread_id: None }
    }

    pub fn with_alias(room_id: impl Into<String>, text: impl Into<String>, alias: impl Into<String>) -> Self {
        Self { room_id: room_id.into(), text: text.into(), alias: Some(alias.into()), thread_id: None }
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

        let alias = first_arg.get("alias").and_then(|v| v.as_str()).map(String::from);

        let file = first_arg
            .get("file")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let files: Vec<FileInfo> = first_arg
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        let attachments: Vec<AttachmentInfo> = first_arg
            .get("attachments")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Some(IncomingMessage {
            msg_id, room_id, room_name, room_fname, sender_name, text, is_dm, timestamp, sender_id, alias,
            file, files, attachments,
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

    #[test]
    fn test_parse_image_attachment() {
        let event = serde_json::json!({
            "msg": "changed", "fields": {
                "eventName": "__my_messages__",
                "args": [
                    {
                        "_id": "m1", "rid": "r1", "msg": "check this",
                        "u": {"_id": "u1", "username": "user1"},
                        "ts": {"$date": 1000i64},
                        "file": {"_id": "f1", "name": "photo.png", "type": "image/png", "size": 1000},
                        "files": [
                            {"_id": "f1", "name": "photo.png", "type": "image/png", "size": 1000, "typeGroup": "image"},
                            {"_id": "f2", "name": "thumb-photo.png", "type": "image/png", "size": 200, "typeGroup": "thumb"}
                        ],
                        "attachments": [
                            {
                                "title": "photo.png",
                                "title_link": "/file-upload/f1/photo.png",
                                "title_link_download": true,
                                "image_url": "/file-upload/f2/photo.png",
                                "image_type": "image/png",
                                "image_size": 1000,
                                "image_dimensions": {"width": 100, "height": 200},
                                "type": "file",
                                "fileId": "f1"
                            }
                        ]
                    },
                    {"roomName": "general", "fname": "General"}
                ]
            }
        });
        let msg = MessageFilter::new("bot").filter(&event).unwrap();

        assert!(msg.file.is_some());
        let f = msg.file.unwrap();
        assert_eq!(f.id, "f1");
        assert_eq!(f.name, "photo.png");
        assert_eq!(f.mime_type, "image/png");
        assert_eq!(f.size, 1000);

        assert_eq!(msg.files.len(), 2);
        assert_eq!(msg.files[0].id, "f1");
        assert_eq!(msg.files[1].id, "f2");

        assert_eq!(msg.attachments.len(), 1);
        let att = &msg.attachments[0];
        assert_eq!(att.title.as_deref(), Some("photo.png"));
        assert_eq!(att.title_link.as_deref(), Some("/file-upload/f1/photo.png"));
        assert_eq!(att.image_url.as_deref(), Some("/file-upload/f2/photo.png"));
        assert_eq!(att.title_link_download, Some(true));
        assert_eq!(att.image_type.as_deref(), Some("image/png"));
        assert_eq!(att.image_size, Some(1000));
        assert!(att.image_dimensions.is_some());
        let dim = att.image_dimensions.as_ref().unwrap();
        assert_eq!(dim.width, 100);
        assert_eq!(dim.height, 200);
        assert_eq!(att.file_id.as_deref(), Some("f1"));
    }

    #[test]
    fn test_parse_message_without_attachments() {
        let event = serde_json::json!({
            "msg": "changed", "fields": {
                "eventName": "room1",
                "args": [
                    {"_id": "m1", "rid": "r1", "msg": "plain text", "u": {"_id": "u1", "username": "user1"}, "ts": {"$date": 1000i64}},
                    {"roomName": "general"}
                ]
            }
        });
        let msg = MessageFilter::new("bot").filter(&event).unwrap();
        assert!(msg.file.is_none());
        assert!(msg.files.is_empty());
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn test_message_filter_parses_alias() {
        let event = serde_json::json!({
            "msg": "changed", "fields": {
                "eventName": "room1",
                "args": [
                    {"_id": "m1", "rid": "r1", "msg": "hello", "u": {"_id": "u1", "username": "user1"}, "alias": "TotallyRealHuman"},
                    {"roomName": "general"}
                ]
            }
        });
        let msg = MessageFilter::new("bot").filter(&event).unwrap();
        assert_eq!(msg.alias.as_deref(), Some("TotallyRealHuman"));
    }

    #[test]
    fn test_message_filter_no_alias_is_none() {
        let event = serde_json::json!({
            "msg": "changed", "fields": {
                "eventName": "room1",
                "args": [
                    {"_id": "m1", "rid": "r1", "msg": "hello", "u": {"_id": "u1", "username": "user1"}},
                    {"roomName": "general"}
                ]
            }
        });
        let msg = MessageFilter::new("bot").filter(&event).unwrap();
        assert_eq!(msg.alias, None);
    }
}
