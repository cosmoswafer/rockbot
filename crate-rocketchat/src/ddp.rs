use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Compute SHA-256 hex digest of a password string.
pub fn sha256_digest(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hex::encode(hasher.finalize())
}

/// Build the DDP connect message.
pub fn connect_message() -> Value {
    json!({
        "msg": "connect",
        "version": "1",
        "support": ["1"]
    })
}

/// Build the DDP login method message with SHA-256 hashed password.
pub fn login_message(username: &str, password: &str) -> Value {
    let digest = sha256_digest(password);
    json!({
        "msg": "method",
        "method": "login",
        "id": "42",
        "params": [{
            "user": { "username": username },
            "password": {
                "digest": digest,
                "algorithm": "sha-256"
            }
        }]
    })
}

/// Build the DDP subscription message for stream-room-messages.
pub fn subscribe_message(sub_id: &str) -> Value {
    json!({
        "msg": "sub",
        "id": sub_id,
        "name": "stream-room-messages",
        "params": ["__my_messages__", false]
    })
}

/// Build a pong response.
pub fn pong_message() -> Value {
    json!({"msg": "pong"})
}

/// Build a sendMessage method call.
pub fn send_message_payload(room_id: &str, text: &str) -> Value {
    json!({
        "msg": "method",
        "method": "sendMessage",
        "id": "42",
        "params": [{
            "rid": room_id,
            "msg": text
        }]
    })
}

/// Build a stream-notify-room (typing indicator) method call.
pub fn typing_payload(room_id: &str, username: &str, is_typing: bool) -> Value {
    json!({
        "msg": "method",
        "method": "stream-notify-room",
        "id": "42",
        "params": [format!("{}/typing", room_id), username, is_typing]
    })
}

/// Extract the `msg` field from a DDP frame.
pub fn msg_field(value: &Value) -> Option<&str> {
    value.get("msg").and_then(|v| v.as_str())
}

/// Extract user ID and token from a login result.
pub fn extract_login_result(value: &Value) -> Option<(String, String)> {
    let result = value.get("result")?;
    let user_id = result.get("id")?.as_str()?.to_string();
    let token = result.get("token")?.as_str()?.to_string();
    Some((user_id, token))
}

/// Check if a DDP message is a "ready" event.
pub fn is_ready(value: &Value) -> bool {
    msg_field(value) == Some("ready")
}

/// Check if a DDP message is a "nosub" event.
pub fn is_nosub(value: &Value) -> bool {
    msg_field(value) == Some("nosub")
}

/// Check if a DDP message is a "connected" event.
pub fn is_connected(value: &Value) -> bool {
    msg_field(value) == Some("connected")
}

/// Check if a DDP message is a "ping" event.
pub fn is_ping(value: &Value) -> bool {
    msg_field(value) == Some("ping")
}

/// Check if a DDP message is a "changed" event.
pub fn is_changed(value: &Value) -> bool {
    msg_field(value) == Some("changed")
}

/// Check if a DDP message is a "result" event.
pub fn is_result(value: &Value) -> bool {
    msg_field(value) == Some("result")
}

/// Extract the `subs` array from a DDP "ready" or "nosub" event to
/// identify which subscription the event is for.
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
        // The password from the DFD example
        let password = "hello";
        let digest = sha256_digest(password);
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
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
        assert_eq!(msg["params"][0]["rid"], "room123");
        assert_eq!(msg["params"][0]["msg"], "hello!");
    }

    #[test]
    fn test_typing_payload() {
        let msg = typing_payload("room123", "user1", true);
        assert_eq!(msg["msg"], "method");
        assert_eq!(msg["method"], "stream-notify-room");
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
            "id": "42",
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
        let v = serde_json::json!({"msg": "ready", "subs": ["ROCKROOMS"]});
        assert_eq!(subs_list(&v), vec!["ROCKROOMS"]);
        let v = serde_json::json!({"msg": "ready", "subs": ["A", "B"]});
        assert_eq!(subs_list(&v), vec!["A", "B"]);
        let v = serde_json::json!({"msg": "ready"});
        assert!(subs_list(&v).is_empty());
    }
}
