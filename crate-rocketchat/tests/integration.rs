use rocketchat::{IncomingMessage, MessageFilter};
use serde_json::json;
use std::collections::HashMap;

fn make_changed_channel(
    msg_id: &str,
    rid: &str,
    room_name: &str,
    text: &str,
    sender_name: &str,
    sender_id: &str,
) -> serde_json::Value {
    json!({
        "msg": "changed",
        "collection": "stream-room-messages",
        "id": msg_id,
        "fields": {
            "eventName": rid,
            "args": [
                {
                    "_id": msg_id,
                    "rid": rid,
                    "msg": text,
                    "u": {
                        "_id": sender_id,
                        "username": sender_name
                    },
                    "ts": {"$date": 1480377601000i64}
                },
                {
                    "roomName": room_name,
                    "fname": "",
                    "roomType": "c"
                }
            ]
        }
    })
}

fn make_changed_dm(
    msg_id: &str,
    rid: &str,
    text: &str,
    sender_name: &str,
    sender_id: &str,
) -> serde_json::Value {
    json!({
        "msg": "changed",
        "collection": "stream-room-messages",
        "id": msg_id,
        "fields": {
            "eventName": rid,
            "args": [
                {
                    "_id": msg_id,
                    "rid": rid,
                    "msg": text,
                    "u": {
                        "_id": sender_id,
                        "username": sender_name
                    }
                }
            ]
        }
    })
}

#[test]
fn test_filter_channel_message_with_fname() {
    let bot_id = "bot123";

    let event = json!({
        "msg": "changed",
        "collection": "stream-room-messages",
        "id": "msg1",
        "fields": {
            "eventName": "room1",
            "args": [
                {
                    "_id": "msg1",
                    "rid": "room1",
                    "msg": "@rockbot hello!",
                    "u": {
                        "_id": "user456",
                        "username": "user1"
                    },
                    "ts": {"$date": 1480377601000i64}
                },
                {
                    "roomName": "shit",
                    "fname": "💩💩💩SHIT屎",
                    "roomType": "c"
                }
            ]
        }
    });

    let filter = MessageFilter::new(bot_id);
    let msg = filter.filter(&event).expect("Should parse message");

    assert_eq!(msg.room_name, "shit");
    assert_eq!(msg.room_fname, "💩💩💩SHIT屎");
    assert!(!msg.is_dm);
}

#[test]
fn test_filter_channel_message_no_fname() {
    let bot_id = "bot123";

    let event = json!({
        "msg": "changed",
        "collection": "stream-room-messages",
        "id": "msg1",
        "fields": {
            "eventName": "room1",
            "args": [
                {
                    "_id": "msg1",
                    "rid": "room1",
                    "msg": "@rockbot hello!",
                    "u": {
                        "_id": "user456",
                        "username": "user1"
                    },
                    "ts": {"$date": 1480377601000i64}
                },
                {
                    "roomName": "general"
                }
            ]
        }
    });

    let filter = MessageFilter::new(bot_id);
    let msg = filter.filter(&event).expect("Should parse message");

    assert_eq!(msg.room_name, "general");
    assert_eq!(msg.room_fname, "");
    assert!(!msg.is_dm);
}

#[test]
fn test_filter_skips_own_messages() {
    let bot_id = "bot123";

    let event = make_changed_channel(
        "msg1", "room1", "general", "hello from bot", "rockbot", "bot123",
    );

    let filter = MessageFilter::new(bot_id);
    assert!(filter.filter(&event).is_none(), "Should skip own messages");
}

#[test]
fn test_filter_direct_message() {
    let bot_id = "bot123";

    let event = make_changed_dm("msg1", "dmRoom1", "hello!", "user1", "user456");

    let filter = MessageFilter::new(bot_id);
    let msg = filter.filter(&event).expect("Should not filter DM");

    assert_eq!(msg.room_id, "dmRoom1");
    assert_eq!(msg.sender_name, "user1");
    assert_eq!(msg.text, "hello!");
    assert!(msg.is_dm);
    assert!(msg.room_name.is_empty());
}

#[test]
fn test_filter_registered_room() {
    let bot_id = "bot123";
    let bot_name = "@rockbot";
    let mut rooms = HashMap::new();
    rooms.insert("secret-room".to_string(), true);

    let event = make_changed_channel(
        "msg1", "room1", "secret-room", "hello there", "user1", "user456",
    );

    let filter = MessageFilter::new(bot_id);
    let msg = filter.filter(&event).expect("Should accept registered room msg");

    assert_eq!(msg.room_name, "secret-room");
    assert!(MessageFilter::is_dm_or_mention(&msg, bot_name, &rooms));
}

#[test]
fn test_filter_non_registered_channel_no_mention() {
    let bot_id = "bot123";
    let bot_name = "@rockbot";
    let rooms: HashMap<String, bool> = HashMap::new();

    let event = make_changed_channel(
        "msg1", "room1", "general", "random chat", "user1", "user456",
    );

    let filter = MessageFilter::new(bot_id);
    let msg = filter.filter(&event).expect("Should parse but not dispatch");

    assert!(
        !MessageFilter::is_dm_or_mention(&msg, bot_name, &rooms),
        "Should not dispatch non-mention in non-registered room"
    );
}

#[test]
fn test_strip_mention() {
    assert_eq!(
        MessageFilter::strip_mention("@rockbot hello", "@rockbot"),
        "hello"
    );
    assert_eq!(MessageFilter::strip_mention("@rockbot", "@rockbot"), "");
    assert_eq!(
        MessageFilter::strip_mention("no mention here", "@rockbot"),
        "no mention here"
    );
}

#[test]
fn test_parse_message_timestamp() {
    let event = make_changed_channel(
        "msg1", "room1", "general", "@rockbot hi", "user1", "user456",
    );

    let bot_id = "bot123";

    let filter = MessageFilter::new(bot_id);
    let msg = filter.filter(&event).unwrap();

    assert_eq!(msg.timestamp, Some(1480377601000));
}

#[test]
fn test_ddp_connect_message() {
    let msg = rocketchat::ddp::connect_message();
    assert_eq!(msg["msg"], "connect");
    assert_eq!(msg["version"], "1");
}

#[test]
fn test_ddp_login_message_hashed() {
    let msg = rocketchat::ddp::login_message("testuser", "secret");
    let params = &msg["params"][0];
    assert_eq!(params["user"]["username"], "testuser");
    assert_eq!(params["password"]["algorithm"], "sha-256");
    let digest = params["password"]["digest"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_config_from_toml() {
    let toml_str = r#"
[server]
url = "chat.example.com"
username = "rockbot"
password = "secret"
"#;
    let config: rocketchat::RocketChatConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.server.url, "chat.example.com");
    assert_eq!(config.server.username, "rockbot");
    assert_eq!(config.server.password, "secret");
    assert!(config.server.use_tls);

    let uri = config.ws_uri().unwrap();
    assert_eq!(uri, "wss://chat.example.com/websocket");
}

#[test]
fn test_config_use_tls_false() {
    let toml_str = r#"
[server]
url = "localhost:3000"
username = "bot"
password = "pw"
use_tls = false
"#;
    let config: rocketchat::RocketChatConfig = toml::from_str(toml_str).unwrap();
    assert!(!config.server.use_tls);
    assert_eq!(config.ws_uri().unwrap(), "ws://localhost:3000/websocket");
}

#[test]
fn test_config_url_strips_protocol() {
    let toml_str = r#"
[server]
url = "https://chat.example.com"
username = "bot"
password = "pw"
"#;
    let config: rocketchat::RocketChatConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.ws_uri().unwrap(), "wss://chat.example.com/websocket");
    assert_eq!(config.host(), "chat.example.com");
}

#[test]
fn test_send_message_payload() {
    let msg = rocketchat::ddp::send_message_payload("room1", "hello world");
    assert_eq!(msg["msg"], "method");
    assert_eq!(msg["method"], "sendMessage");
    assert_eq!(msg["params"][0]["rid"], "room1");
    assert_eq!(msg["params"][0]["msg"], "hello world");
    assert!(msg["params"][0].get("alias").is_none());
    // _id is timestamp-seq format
    let id_str = msg["params"][0]["_id"].as_str().unwrap();
    assert!(id_str.contains('-'), "_id should be timestamp-seq format");
}

#[test]
fn test_send_message_payload_with_alias() {
    let msg = rocketchat::ddp::send_message_payload_with_alias("room1", "hello world", Some("CustomBot"));
    assert_eq!(msg["msg"], "method");
    assert_eq!(msg["method"], "sendMessage");
    assert_eq!(msg["params"][0]["rid"], "room1");
    assert_eq!(msg["params"][0]["msg"], "hello world");
    assert_eq!(msg["params"][0]["alias"], "CustomBot");
    // _id is timestamp-seq format
    let id_str = msg["params"][0]["_id"].as_str().unwrap();
    assert!(id_str.contains('-'), "_id should be timestamp-seq format");
}

#[test]
fn test_create_direct_message_payload() {
    let msg = rocketchat::ddp::create_direct_message_payload("targetuser");
    assert_eq!(msg["msg"], "method");
    assert_eq!(msg["method"], "createDirectMessage");
    assert!(msg["id"].as_str().unwrap().parse::<u64>().is_ok());
    assert_eq!(msg["params"][0], "targetuser");
}

#[test]
fn test_set_real_name_payload() {
    let msg = rocketchat::ddp::set_real_name_payload("MyNewName");
    assert_eq!(msg["msg"], "method");
    assert_eq!(msg["method"], "setRealName");
    assert!(msg["id"].as_str().unwrap().parse::<u64>().is_ok());
    assert_eq!(msg["params"][0], "MyNewName");
}

#[test]
fn test_subscribe_payload() {
    let msg = rocketchat::ddp::subscribe_message("ABCROCK");
    assert_eq!(msg["msg"], "sub");
    assert_eq!(msg["id"], "ABCROCK");
    assert_eq!(msg["name"], "stream-room-messages");
    assert_eq!(msg["params"][0], "__my_messages__");
    assert_eq!(msg["params"][1], false);
}

#[test]
fn test_typing_payload_true() {
    let msg = rocketchat::ddp::typing_payload("room123", "botuser", true);
    assert_eq!(msg["params"][0], "room123/user-activity");
    assert_eq!(msg["params"][1], "botuser");
    let activities = msg["params"][2].as_array().unwrap();
    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0], "user-typing");
    assert!(msg["params"][3].is_object());
}

#[test]
fn test_typing_payload_false() {
    let msg = rocketchat::ddp::typing_payload("room123", "botuser", false);
    assert_eq!(msg["params"][0], "room123/user-activity");
    assert_eq!(msg["params"][1], "botuser");
    let activities = msg["params"][2].as_array().unwrap();
    assert!(activities.is_empty());
    assert!(msg["params"][3].is_object());
}

#[test]
fn test_extract_login_result_valid() {
    let v = json!({
        "msg": "result",
        "id": "42",
        "result": {
            "id": "user-abc",
            "token": "token-xyz"
        }
    });
    let (uid, token) = rocketchat::ddp::extract_login_result(&v).unwrap();
    assert_eq!(uid, "user-abc");
    assert_eq!(token, "token-xyz");
}

#[test]
fn test_extract_login_result_missing_fields() {
    let v = json!({"msg": "result", "result": {}});
    assert!(rocketchat::ddp::extract_login_result(&v).is_none());

    let v = json!({"msg": "result"});
    assert!(rocketchat::ddp::extract_login_result(&v).is_none());
}

#[test]
fn test_msg_field_helper() {
    assert_eq!(
        rocketchat::ddp::msg_field(&json!({"msg": "ping"})),
        Some("ping")
    );
    assert_eq!(rocketchat::ddp::msg_field(&json!({})), None);
}

#[test]
fn test_dispatch_checks() {
    let ping = json!({"msg": "ping"});
    let pong = json!({"msg": "pong"});
    let connected = json!({"msg": "connected"});
    let result = json!({"msg": "result"});
    let changed = json!({"msg": "changed"});
    let ready = json!({"msg": "ready"});
    let nosub = json!({"msg": "nosub"});

    assert!(rocketchat::ddp::is_ping(&ping));
    assert!(rocketchat::ddp::msg_field(&pong) == Some("pong"));
    assert!(rocketchat::ddp::is_connected(&connected));
    assert!(rocketchat::ddp::is_result(&result));
    assert!(rocketchat::ddp::is_changed(&changed));
    assert!(rocketchat::ddp::is_ready(&ready));
    assert!(rocketchat::ddp::is_nosub(&nosub));

    assert!(!rocketchat::ddp::is_ping(&changed));
    assert!(!rocketchat::ddp::is_changed(&ping));
}

#[test]
fn test_bot_reply_new() {
    let reply = rocketchat::BotReply::new("room1", "hello");
    assert_eq!(reply.room_id, "room1");
    assert_eq!(reply.text, "hello");
    assert_eq!(reply.thread_id, None);
    assert_eq!(reply.alias, None);
}

#[test]
fn test_bot_reply_with_alias() {
    let reply = rocketchat::BotReply::with_alias("room1", "hello", "ImposterBot");
    assert_eq!(reply.room_id, "room1");
    assert_eq!(reply.text, "hello");
    assert_eq!(reply.alias.as_deref(), Some("ImposterBot"));
    assert_eq!(reply.thread_id, None);
}

#[test]
fn test_incoming_message_dm_detection() {
    let msg = IncomingMessage {
        msg_id: Some("m1".into()),
        room_id: "rid1".into(),
        room_name: "".into(),
        room_fname: "".into(),
        sender_name: "user".into(),
        text: "hi".into(),
        is_dm: true,
        timestamp: None,
        sender_id: "uid".into(),
        alias: None,
        file: None,
        files: vec![],
        attachments: vec![],
    };

    let rooms: HashMap<String, bool> = HashMap::new();
    let bot_name = "@rockbot";

    assert!(MessageFilter::is_dm_or_mention(&msg, bot_name, &rooms));

    let msg2 = IncomingMessage {
        msg_id: Some("m2".into()),
        room_id: "rid2".into(),
        room_name: "general".into(),
        room_fname: "".into(),
        sender_name: "user".into(),
        text: "hello".into(),
        is_dm: false,
        timestamp: None,
        sender_id: "uid".into(),
        alias: None,
        file: None,
        files: vec![],
        attachments: vec![],
    };
    assert!(!MessageFilter::is_dm_or_mention(&msg2, bot_name, &rooms));

    let msg3 = IncomingMessage {
        msg_id: Some("m3".into()),
        room_id: "rid3".into(),
        room_name: "general".into(),
        room_fname: "".into(),
        sender_name: "user".into(),
        text: "@rockbot help".into(),
        is_dm: false,
        timestamp: None,
        sender_id: "uid".into(),
        alias: None,
        file: None,
        files: vec![],
        attachments: vec![],
    };
    assert!(MessageFilter::is_dm_or_mention(&msg3, bot_name, &rooms));
}

#[test]
fn test_registered_room_dispatch() {
    let mut rooms = HashMap::new();
    rooms.insert("ops-room".into(), true);

    let msg = IncomingMessage {
        msg_id: Some("m1".into()),
        room_id: "rid1".into(),
        room_name: "ops-room".into(),
        room_fname: "".into(),
        sender_name: "user".into(),
        text: "deploy now".into(),
        is_dm: false,
        timestamp: None,
        sender_id: "uid".into(),
        alias: None,
        file: None,
        files: vec![],
        attachments: vec![],
    };

    assert!(MessageFilter::is_dm_or_mention(&msg, "@rockbot", &rooms));
}

#[test]
fn test_sha256_digest_known_value() {
    let digest = rocketchat::ddp::sha256_digest("hello");
    assert_eq!(
        digest,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn test_client_new() {
    let config = rocketchat::RocketChatConfig {
        server: rocketchat::ServerConfig {
            url: "chat.example.com".into(),
            username: "bot".into(),
            password: "pw".into(),
            use_tls: true,
        },
    };
    let client = rocketchat::RocketChatClient::new(config);
    assert_eq!(client.bot_name(), "@bot");
    assert!(client.user_id().is_none());
}
