use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static MSG_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    MSG_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

fn unique_msg_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = MSG_ID.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", ts, seq)
}

pub fn sha256_digest(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn connect_message() -> Value {
    json!({
        "msg": "connect",
        "version": "1",
        "support": ["1"]
    })
}

pub fn login_message(username: &str, password: &str) -> Value {
    let digest = sha256_digest(password);
    json!({
        "msg": "method",
        "method": "login",
        "id": next_id(),
        "params": [{
            "user": { "username": username },
            "password": {
                "digest": digest,
                "algorithm": "sha-256"
            }
        }]
    })
}

pub fn subscribe_message(sub_id: &str) -> Value {
    json!({
        "msg": "sub",
        "id": sub_id,
        "name": "stream-room-messages",
        "params": ["__my_messages__", false]
    })
}

pub fn pong_message() -> Value {
    json!({"msg": "pong"})
}

pub fn send_message_payload(room_id: &str, text: &str) -> Value {
    send_message_payload_with_alias(room_id, text, None)
}

pub fn send_message_payload_with_alias(room_id: &str, text: &str, alias: Option<&str>) -> Value {
    let mut params = json!({
        "_id": unique_msg_id(),
        "rid": room_id,
        "msg": text,
    });
    if let Some(a) = alias {
        params["alias"] = serde_json::Value::String(a.to_string());
    }
    json!({
        "msg": "method",
        "method": "sendMessage",
        "id": next_id(),
        "params": [params]
    })
}

pub fn create_direct_message_payload(username: &str) -> Value {
    json!({
        "msg": "method",
        "method": "createDirectMessage",
        "id": next_id(),
        "params": [username]
    })
}

pub fn set_real_name_payload(name: &str) -> Value {
    json!({
        "msg": "method",
        "method": "setRealName",
        "id": next_id(),
        "params": [name]
    })
}

pub fn typing_payload(room_id: &str, username: &str, is_typing: bool) -> Value {
    json!({
        "msg": "method",
        "method": "stream-notify-room",
        "id": next_id(),
        "params": [format!("{}/typing", room_id), username, is_typing]
    })
}

pub fn msg_field(value: &Value) -> Option<&str> {
    value.get("msg").and_then(|v| v.as_str())
}

pub fn extract_login_result(value: &Value) -> Option<(String, String)> {
    let result = value.get("result")?;
    let user_id = result.get("id")?.as_str()?.to_string();
    let token = result.get("token")?.as_str()?.to_string();
    Some((user_id, token))
}

pub fn is_ready(value: &Value) -> bool {
    msg_field(value) == Some("ready")
}

pub fn is_nosub(value: &Value) -> bool {
    msg_field(value) == Some("nosub")
}

pub fn is_connected(value: &Value) -> bool {
    msg_field(value) == Some("connected")
}

pub fn is_ping(value: &Value) -> bool {
    msg_field(value) == Some("ping")
}

pub fn is_changed(value: &Value) -> bool {
    msg_field(value) == Some("changed")
}

pub fn is_result(value: &Value) -> bool {
    msg_field(value) == Some("result")
}

pub fn subs_list(value: &Value) -> Vec<String> {
    value
        .get("subs")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_digest() {
        let digest = sha256_digest("hello");
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_digest_known() {
        let password = "hello";
        let digest = sha256_digest(password);
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_next_id_increments() {
        let a = next_id();
        let b = next_id();
        let c = next_id();
        let a_num: u64 = a.parse().unwrap();
        let b_num: u64 = b.parse().unwrap();
        let c_num: u64 = c.parse().unwrap();
        assert_eq!(b_num, a_num + 1, "IDs must be sequential");
        assert_eq!(c_num, b_num + 1, "IDs must be sequential");
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn test_next_id_unique_across_payloads() {
        let login = login_message("u", "p");
        let typing = typing_payload("r", "u", true);
        let send = send_message_payload("r", "hello");
        let login_id: u64 = login["id"].as_str().unwrap().parse().unwrap();
        let typing_id: u64 = typing["id"].as_str().unwrap().parse().unwrap();
        let send_id: u64 = send["id"].as_str().unwrap().parse().unwrap();
        assert_ne!(login_id, typing_id, "login and typing must have different ids");
        assert_ne!(typing_id, send_id, "typing and send must have different ids");
        assert_ne!(login_id, send_id, "login and send must have different ids");
        // _id is timestamp-seq format, method id is numeric
        let msg_id_str = send["params"][0]["_id"].as_str().unwrap();
        assert!(msg_id_str.contains('-'), "message _id should be timestamp-seq format");
        let method_id: u64 = send["id"].as_str().unwrap().parse().unwrap();
        assert_ne!(method_id.to_string().as_str(), msg_id_str);
    }

    #[test]
    fn test_connect_message() {
        let msg = connect_message();
        assert_eq!(msg["msg"], "connect");
        assert_eq!(msg["version"], "1");
        assert_eq!(msg["support"][0], "1");
    }

    #[test]
    fn test_login_message_has_digest() {
        let msg = login_message("testuser", "testpass");
        assert_eq!(msg["msg"], "method");
        assert_eq!(msg["method"], "login");
        assert!(msg["id"].as_str().unwrap().parse::<u64>().is_ok());
        let params = &msg["params"][0];
        assert_eq!(params["user"]["username"], "testuser");
        let pw = &params["password"];
        assert_eq!(pw["algorithm"], "sha-256");
        assert!(!pw["digest"].as_str().unwrap().is_empty());
    }

    #[test]
    fn test_subscribe_message() {
        let msg = subscribe_message("ABCROCK");
        assert_eq!(msg["msg"], "sub");
        assert_eq!(msg["id"], "ABCROCK");
        assert_eq!(msg["name"], "stream-room-messages");
        assert_eq!(msg["params"][0], "__my_messages__");
        assert_eq!(msg["params"][1], false);
    }

    #[test]
    fn test_pong_message() {
        let msg = pong_message();
        assert_eq!(msg["msg"], "pong");
    }

    #[test]
    fn test_send_message_payload() {
        let msg = send_message_payload("room123", "hello!");
        assert_eq!(msg["msg"], "method");
        assert_eq!(msg["method"], "sendMessage");
        assert!(msg["id"].as_str().unwrap().parse::<u64>().is_ok());
        // _id is now timestamp-seq format, e.g. "1781112318307-1"
        let id_str = msg["params"][0]["_id"].as_str().unwrap();
        assert!(id_str.contains('-'), "_id should contain timestamp-seq separator");
        assert_eq!(msg["params"][0]["rid"], "room123");
        assert_eq!(msg["params"][0]["msg"], "hello!");
    }

    #[test]
    fn test_typing_payload() {
        let msg = typing_payload("room123", "user1", true);
        assert_eq!(msg["msg"], "method");
        assert_eq!(msg["method"], "stream-notify-room");
        assert!(msg["id"].as_str().unwrap().parse::<u64>().is_ok());
        assert_eq!(msg["params"][0], "room123/typing");
        assert_eq!(msg["params"][1], "user1");
        assert_eq!(msg["params"][2], true);
    }

    #[test]
    fn test_msg_field() {
        let v = serde_json::json!({"msg": "ping"});
        assert_eq!(msg_field(&v), Some("ping"));
        let v = serde_json::json!({"foo": "bar"});
        assert_eq!(msg_field(&v), None);
    }

    #[test]
    fn test_extract_login_result() {
        let v = serde_json::json!({
            "msg": "result",
            "id": "1",
            "result": {
                "id": "user-id-123",
                "token": "auth-token-abc",
                "tokenExpires": {"$date": 1480377601}
            }
        });
        let (uid, token) = extract_login_result(&v).unwrap();
        assert_eq!(uid, "user-id-123");
        assert_eq!(token, "auth-token-abc");
    }

    #[test]
    fn test_extract_login_result_no_result_field() {
        let v = serde_json::json!({"msg": "changed"});
        assert!(extract_login_result(&v).is_none());
    }

    #[test]
    fn test_dispatch_checks() {
        assert!(is_ping(&serde_json::json!({"msg": "ping"})));
        assert!(is_pong_msg(&serde_json::json!({"msg": "pong"})));
        assert!(is_connected(&serde_json::json!({"msg": "connected"})));
        assert!(is_result(&serde_json::json!({"msg": "result"})));
        assert!(is_changed(&serde_json::json!({"msg": "changed"})));
        assert!(is_ready(&serde_json::json!({"msg": "ready"})));
        assert!(is_nosub(&serde_json::json!({"msg": "nosub"})));
        assert!(!is_ping(&serde_json::json!({"msg": "changed"})));
    }

    fn is_pong_msg(value: &Value) -> bool {
        msg_field(value) == Some("pong")
    }

    #[test]
    fn test_subs_list() {
        let v = serde_json::json!({"msg": "ready", "subs": ["ABCROCK"]});
        assert_eq!(subs_list(&v), vec!["ABCROCK"]);
        let v = serde_json::json!({"msg": "ready", "subs": ["A", "B"]});
        assert_eq!(subs_list(&v), vec!["A", "B"]);
        let v = serde_json::json!({"msg": "ready"});
        assert!(subs_list(&v).is_empty());
    }
}
