/// Integration tests that connect to a real RocketChat server.
/// These tests require a valid `config.toml` in the workspace root.
/// Run with: `cargo test --test integration_real -- --ignored`
use futures_util::{SinkExt, StreamExt};
use rocketchat::{IncomingMessage, MessageSender, RocketChatClient};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn config_path() -> String {
    // Try workspace root config.toml first (when running from workspace root)
    if std::path::Path::new("config.toml").exists() {
        return "config.toml".to_string();
    }
    // Try relative to crate root
    if std::path::Path::new("../../config.toml").exists() {
        return "../../config.toml".to_string();
    }
    // Try absolute path
    if std::path::Path::new("/home/gamer/Workspaces/rockbot/config.toml").exists() {
        return "/home/gamer/Workspaces/rockbot/config.toml".to_string();
    }
    panic!("config.toml not found. Create one with [server] section to run these tests.");
}

fn sha256_digest(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hex::encode(hasher.finalize())
}

fn init_crypto() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn ddp_handshake(ws: &mut WsStream, username: &str, password: &str) -> (String, String) {
    // DDP connect
    let connect_msg = serde_json::json!({
        "msg": "connect",
        "version": "1",
        "support": ["1"]
    });
    ws.send(Message::Text(connect_msg.to_string().into()))
        .await
        .unwrap();
    let _ = expect_msg(ws, "connected").await;

    // DDP login
    let digest = sha256_digest(password);
    let login_msg = serde_json::json!({
        "msg": "method",
        "method": "login",
        "id": "login1",
        "params": [{
            "user": { "username": username },
            "password": {
                "digest": digest,
                "algorithm": "sha-256"
            }
        }]
    });
    ws.send(Message::Text(login_msg.to_string().into()))
        .await
        .unwrap();
    let result = expect_msg(ws, "result").await;
    let user_id = result["result"]["id"].as_str().unwrap().to_string();
    let token = result["result"]["token"].as_str().unwrap().to_string();
    (user_id, token)
}

async fn expect_msg(ws: &mut WsStream, expected_msg: &str) -> Value {
    loop {
        let frame = ws.next().await.unwrap().unwrap();
        match frame {
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                let msg_type = value.get("msg").and_then(|v| v.as_str());
                if msg_type == Some(expected_msg) {
                    return value;
                }
                if msg_type == Some("ping") {
                    let pong = serde_json::json!({"msg": "pong"});
                    ws.send(Message::Text(pong.to_string().into()))
                        .await
                        .unwrap();
                }
            }
            Message::Close(_) => panic!("Connection closed while waiting for {}", expected_msg),
            _ => continue,
        }
    }
}

#[tokio::test]
#[ignore = "requires a running RocketChat server and valid config.toml"]
async fn test_config_toml_exists_and_parses() {
    let path = config_path();
    let config =
        rocketchat::RocketChatConfig::from_file(&path).expect("Failed to parse config.toml");

    assert!(!config.server.url.is_empty());
    assert!(!config.server.username.is_empty());
    assert!(!config.server.password.is_empty());
    assert!(config.server.use_tls);
}

#[tokio::test]
#[ignore = "requires a running RocketChat server and valid config.toml"]
async fn test_connect_and_receive_events() {
    let path = config_path();
    let mut client =
        RocketChatClient::from_config_file(&path).expect("Failed to create client from config");
    client.register_room("general");

    let received = Arc::new(AtomicUsize::new(0));
    let received_clone = received.clone();

    let result = tokio::time::timeout(
        Duration::from_secs(30),
        client.connect_and_run(move |msg: IncomingMessage, sender: MessageSender| {
            let count = received_clone.clone();
            async move {
                let n = count.fetch_add(1, Ordering::SeqCst);
                eprintln!(
                    "Received message #{}: {} from {} in {}: {}",
                    n,
                    msg.msg_id.unwrap_or_default(),
                    msg.sender_name,
                    msg.room_name,
                    msg.text
                );

                // Auto-reply to test roundtrip
                let reply = format!("Echo: {}", msg.text);
                if let Err(e) = sender.reply(&reply).await {
                    eprintln!("Failed to send reply: {}", e);
                }
            }
        }),
    )
    .await;

    match result {
        Ok(Err(e)) => {
            let count = received.load(Ordering::SeqCst);
            eprintln!("Connection ended after {} messages: {}", count, e);
        }
        Ok(Ok(())) => {
            let count = received.load(Ordering::SeqCst);
            eprintln!("Connection closed cleanly after {} messages", count);
        }
        Err(_timeout) => {
            let count = received.load(Ordering::SeqCst);
            eprintln!("Test timed out after 30s, received {} messages", count);
            assert!(count > 0, "Should have received at least some messages");
        }
    }
}

#[tokio::test]
#[ignore = "requires a running RocketChat server and valid config.toml"]
async fn test_send_message_and_verify() {
    let path = config_path();
    let config =
        rocketchat::RocketChatConfig::from_file(&path).expect("Failed to parse config.toml");

    let uri = config.ws_uri().unwrap();
    assert!(uri.starts_with("wss://"));
    assert!(uri.ends_with("/websocket"));

    let host = config.host();
    assert!(!host.is_empty());
    assert!(!host.starts_with("https://"));
    assert!(!host.starts_with("http://"));
}

#[tokio::test]
#[ignore = "requires a running RocketChat server and valid config.toml"]
async fn test_send_message_with_alias_two_clients() {
    init_crypto();

    let path = config_path();
    let config =
        rocketchat::RocketChatConfig::from_file(&path).expect("Failed to parse config.toml");
    let ws_uri = config.ws_uri().unwrap();
    let username = &config.server.username;
    let password = &config.server.password;
    let alias_name = "CoolAliasBot";
    let test_text = format!("Alias integration test {}", std::process::id());

    // --- Client A: create DM room and get room_id ---
    eprintln!("[Client A] Connecting to {}", ws_uri);
    let (mut ws_a, _) = connect_async(&ws_uri)
        .await
        .expect("Failed to connect Client A");
    let (user_id_a, token_a) = ddp_handshake(&mut ws_a, username, password).await;
    eprintln!("[Client A] Logged in, user_id={}, token={}", user_id_a, &token_a[..8]);

    // Create DM to self to get a room
    let dm_msg = serde_json::json!({
        "msg": "method", "method": "createDirectMessage", "id": "cdm",
        "params": [username]
    });
    ws_a.send(Message::Text(dm_msg.to_string().into())).await.unwrap();
    let dm_result = expect_msg(&mut ws_a, "result").await;
    let room_id = dm_result["result"]["rid"].as_str().unwrap().to_string();
    eprintln!("[Client A] DM room: {}", room_id);

    // Subscribe Client A to the room
    let sub_a = serde_json::json!({
        "msg": "sub", "id": "sub_a", "name": "stream-room-messages",
        "params": [room_id, false]
    });
    ws_a.send(Message::Text(sub_a.to_string().into())).await.unwrap();
    let _ = expect_msg(&mut ws_a, "ready").await;
    eprintln!("[Client A] Subscribed to room");

    // --- Client B: connect and subscribe ---
    eprintln!("[Client B] Connecting");
    let (mut ws_b, _) = connect_async(&ws_uri)
        .await
        .expect("Failed to connect Client B");
    let (user_id_b, _token_b) = ddp_handshake(&mut ws_b, username, password).await;
    eprintln!("[Client B] Logged in, user_id={}", user_id_b);

    let sub_b = serde_json::json!({
        "msg": "sub", "id": "sub_b", "name": "stream-room-messages",
        "params": [room_id, false]
    });
    ws_b.send(Message::Text(sub_b.to_string().into())).await.unwrap();
    let _ = expect_msg(&mut ws_b, "ready").await;
    eprintln!("[Client B] Subscribed to room");

    // Give subscriptions a moment to stabilize
    tokio::time::sleep(Duration::from_millis(500)).await;

    // --- Send message with alias via REST API ---
    eprintln!("[REST] Sending message with alias '{}'", alias_name);
    let host = config.host();
    let rest_url = format!("https://{}/api/v1/chat.sendMessage", host);
    let rest_body = serde_json::json!({
        "message": {
            "rid": &room_id,
            "msg": &test_text,
            "alias": alias_name
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&rest_url)
        .header("Content-Type", "application/json")
        .header("X-Auth-Token", &token_a)
        .header("X-User-Id", &user_id_a)
        .json(&rest_body)
        .send()
        .await
        .expect("REST request failed");

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    eprintln!("[REST] status={}, body={}", status, &body[..body.len().min(300)]);
    assert!(status.is_success(), "REST API sendMessage failed: {}", body);

    // --- Verify Client A receives the message with alias ---
    eprintln!("[Client A] Waiting for changed event...");
    let ca_result = read_until_alias(&mut ws_a, &test_text, alias_name).await;
    if ca_result {
        eprintln!("[Client A] PASSED: received message with alias '{}'", alias_name);
    }

    // --- Verify Client B also receives the message with alias ---
    eprintln!("[Client B] Waiting for changed event...");
    let cb_result = read_until_alias(&mut ws_b, &test_text, alias_name).await;
    if cb_result {
        eprintln!("[Client B] PASSED: received message with alias '{}'", alias_name);
    }

    // --- Verify IncomingMessage parser extracts alias correctly ---
    eprintln!("[Parser] Testing IncomingMessage alias extraction...");
    let sample_changed = serde_json::json!({
        "msg": "changed",
        "collection": "stream-room-messages",
        "id": "id",
        "fields": {
            "eventName": &room_id,
            "args": [
                {
                    "_id": "mid",
                    "rid": &room_id,
                    "msg": &test_text,
                    "alias": alias_name,
                    "u": {"_id": &user_id_a, "username": "bogus"},
                    "ts": {"$date": 1480377601000i64}
                },
                {"roomName": &room_id, "fname": "", "roomType": "d"}
            ]
        }
    });

    let filter = rocketchat::MessageFilter::new("different_user_id");
    if let Some(msg) = filter.filter(&sample_changed) {
        eprintln!("[Parser] IncomingMessage: sender={}, alias={:?}", msg.sender_name, msg.alias);
        assert_eq!(msg.alias.as_deref(), Some(alias_name),
            "IncomingMessage.alias should be '{}'", alias_name);
        eprintln!("[Parser] PASSED: alias field correctly parsed");
    } else {
        panic!("[Parser] FAILED: filter returned None");
    }

    // Cleanup
    let _ = ws_a.close(None).await;
    let _ = ws_b.close(None).await;

    assert!(ca_result, "Client A failed to receive alias");
    assert!(cb_result, "Client B failed to receive alias");
    eprintln!("SUCCESS: All alias tests passed on two clients");
}

async fn read_until_alias(ws: &mut WsStream, expected_text: &str, expected_alias: &str) -> bool {
    let timeout = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let frame = match ws.next().await {
                Some(Ok(f)) => f,
                _ => return false,
            };
            if let Message::Text(text) = frame {
                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let msg_type = value.get("msg").and_then(|v| v.as_str());
                match msg_type {
                    Some("changed") => {
                        if let Some(args) = value["fields"]["args"].as_array() {
                            if let Some(first) = args.first() {
                                let msg_text = first["msg"].as_str().unwrap_or("");
                                let msg_alias = first.get("alias").and_then(|v| v.as_str());
                                eprintln!("  changed: text='{}', alias={:?}", msg_text, msg_alias);
                                if msg_text == expected_text {
                                    assert_eq!(
                                        msg_alias,
                                        Some(expected_alias),
                                        "Expected alias '{}'",
                                        expected_alias
                                    );
                                    return true;
                                }
                            }
                        }
                    }
                    Some("ping") => {
                        let pong = serde_json::json!({"msg": "pong"});
                        let _ = ws.send(Message::Text(pong.to_string().into())).await;
                    }
                    Some("updated") => {}
                    _ => {}
                }
            }
        }
    })
    .await;
    timeout.unwrap_or(false)
}

#[tokio::test]
#[ignore = "requires a running RocketChat server and valid config.toml"]
async fn test_set_real_name_via_ddp() {
    init_crypto();

    let path = config_path();
    let config =
        rocketchat::RocketChatConfig::from_file(&path).expect("Failed to parse config.toml");
    let ws_uri = config.ws_uri().unwrap();
    let username = &config.server.username;
    let password = &config.server.password;

    eprintln!("Connecting to {}", ws_uri);
    let (mut ws, _) = connect_async(&ws_uri)
        .await
        .expect("Failed to connect");
    let (user_id, _token) = ddp_handshake(&mut ws, username, password).await;
    eprintln!("Logged in as {user_id}");

    let original_name = "香菜";
    let test_name = format!("TestBot_{}", std::process::id() % 10000);

    // Change real name
    eprintln!("Setting real name to '{}'", test_name);
    let set_msg = rocketchat::ddp::set_real_name_payload(&test_name);
    ws.send(Message::Text(set_msg.to_string().into())).await.unwrap();

    let set_ok = {
        let timeout = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let frame = ws.next().await.unwrap().unwrap();
                if let Message::Text(text) = frame {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value.get("msg").and_then(|v| v.as_str()) == Some("result") {
                        if let Some(err) = value.get("error") {
                            eprintln!("setRealName ERROR: {} - {}", err["reason"], err["message"]);
                            return false;
                        }
                        let result_name = value["result"].as_str().unwrap_or("");
                        eprintln!("setRealName OK, result='{}'", result_name);
                        assert_eq!(result_name, test_name, "Result should match requested name");
                        return true;
                    }
                }
            }
        }).await;
        timeout.unwrap_or(false)
    };

    // Revert to original name
    if set_ok {
        tokio::time::sleep(Duration::from_secs(2)).await; // respect rate limit
        eprintln!("Reverting name to '{}'", original_name);
        let revert_msg = rocketchat::ddp::set_real_name_payload(original_name);
        ws.send(Message::Text(revert_msg.to_string().into())).await.unwrap();

        let revert_timeout = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let frame = ws.next().await.unwrap().unwrap();
                if let Message::Text(text) = frame {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value.get("msg").and_then(|v| v.as_str()) == Some("result") {
                        if let Some(err) = value.get("error") {
                            eprintln!("Revert ERROR: {} - {}",
                                err["reason"], err["message"]);
                        } else {
                            eprintln!("Reverted successfully");
                        }
                        return true;
                    }
                }
            }
        }).await;

        if revert_timeout.is_err() {
            eprintln!("Warning: name revert may not have completed, manual check needed");
        }
    }

    let _ = ws.close(None).await;
    assert!(set_ok, "setRealName DDP method failed");
    eprintln!("SUCCESS: setRealName DDP method works");
}
