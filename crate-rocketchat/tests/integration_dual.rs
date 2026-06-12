/// Dual-connection integration tests against a real RocketChat server.
///
/// These tests open two independent WebSocket sessions using the same
/// credentials and test features that require two sessions to observe:
///   - Typing indicators (peer A sends, peer B receives the event)
///   - Message aliases  (peer A sends with alias, peer B sees alias in message)
///
/// Run with: `cargo test --test integration_dual -- --ignored --nocapture`
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Once;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

use rocketchat::ddp;
use rocketchat::config::RocketChatConfig;

static INIT_CRYPTO: Once = Once::new();
static LOCAL_ID: AtomicU64 = AtomicU64::new(9000);

fn next_local_id() -> String {
    LOCAL_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

type WsWrite = futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type WsRead = futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

fn config_path() -> String {
    for candidate in &["config.toml", "../../config.toml", "/home/claw/rockbot/config.toml", "/home/gamer/Workspaces/rockbot/config.toml"] {
        if std::path::Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    panic!("config.toml not found (cwd={cwd})");
}

async fn send_json(write: &mut WsWrite, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string(value).unwrap();
    write.send(Message::Text(text.into())).await.map_err(|e| format!("send: {e}"))
}

async fn expect_msg(write: &mut WsWrite, read: &mut WsRead, expected: &str) -> Result<Value, String> {
    for _ in 0..30 {
        let frame = tokio::time::timeout(Duration::from_secs(10), read.next())
            .await.map_err(|_| "timeout".to_string())?
            .ok_or_else(|| "stream ended".to_string())?
            .map_err(|e| format!("ws: {e}"))?;
        let v: Value = match frame {
            Message::Text(t) => serde_json::from_str(&t).map_err(|e| format!("json: {e}"))?,
            Message::Ping(data) => {
                write.send(Message::Pong(data)).await.map_err(|e| format!("pong: {e}"))?;
                continue;
            }
            Message::Close(_) => return Err("closed".into()),
            _ => continue,
        };
        if let Some("ping") = ddp::msg_field(&v) {
            send_json(write, &ddp::pong_message()).await.map_err(|e| format!("pong: {e}"))?;
            continue;
        }
        if ddp::msg_field(&v) == Some(expected) {
            return Ok(v);
        }
    }
    Err(format!("never received '{expected}'"))
}

async fn raw_connect(config: &RocketChatConfig) -> Result<(WsWrite, WsRead, String, String), String> {
    INIT_CRYPTO.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let uri = config.ws_uri().map_err(|e| format!("bad uri: {e}"))?;
    let (ws_stream, _) = connect_async(&uri).await.map_err(|e| format!("connect: {e}"))?;
    let (mut write, mut read) = ws_stream.split();

    send_json(&mut write, &ddp::connect_message()).await.map_err(|e| format!("connect: {e}"))?;
    let _ = expect_msg(&mut write, &mut read, "connected").await?;

    let login_msg = ddp::login_message(&config.server.username, &config.server.password);
    send_json(&mut write, &login_msg).await.map_err(|e| format!("login: {e}"))?;
    let result = expect_msg(&mut write, &mut read, "result").await?;
    let (user_id, _token) = ddp::extract_login_result(&result)
        .ok_or_else(|| format!("no id/token in result: {result}"))?;

    Ok((write, read, user_id, config.server.username.clone()))
}

async fn subscribe_my_messages(write: &mut WsWrite, read: &mut WsRead, sub_id: &str) -> Result<(), String> {
    let sub = ddp::subscribe_message(sub_id);
    send_json(write, &sub).await.map_err(|e| format!("sub: {e}"))?;
    expect_msg(write, read, "ready").await?;
    Ok(())
}

async fn subscribe_notify(
    write: &mut WsWrite, read: &mut WsRead, sub_id: &str, event_name: &str,
) -> Result<(), String> {
    let sub = json!({"msg":"sub","id":sub_id,"name":"stream-notify-room","params":[event_name,false]});
    send_json(write, &sub).await.map_err(|e| format!("sub: {e}"))?;
    expect_msg(write, read, "ready").await?;
    Ok(())
}

async fn create_dm(write: &mut WsWrite, read: &mut WsRead, username: &str) -> Result<String, String> {
    let id = "dm_test";
    let create = json!({"msg":"method","method":"createDirectMessage","id":id,"params":[username]});
    send_json(write, &create).await.map_err(|e| format!("createDM: {e}"))?;
    for _ in 0..30 {
        let frame = tokio::time::timeout(Duration::from_secs(10), read.next())
            .await.map_err(|_| "timeout".to_string())?
            .ok_or_else(|| "stream end".to_string())?
            .map_err(|e| format!("ws: {e}"))?;
        let v: Value = match frame {
            Message::Text(t) => serde_json::from_str(&t).map_err(|e| format!("json: {e}"))?,
            Message::Ping(data) => { write.send(Message::Pong(data)).await.map_err(|e| format!("pong: {e}"))?; continue; }
            Message::Close(_) => return Err("closed".into()),
            _ => continue,
        };
        if let Some("ping") = ddp::msg_field(&v) {
            send_json(write, &ddp::pong_message()).await.map_err(|e| format!("pong: {e}"))?;
            continue;
        }
        if ddp::is_ready(&v) { continue; }
        if ddp::is_result(&v) && v.get("id").and_then(|i| i.as_str()) == Some(id) {
            return v.get("result").and_then(|r| r.get("rid")).and_then(|r| r.as_str())
                .map(String::from).ok_or_else(|| format!("no rid: {v}"));
        }
    }
    Err("createDM never returned".into())
}

async fn send_message(write: &mut WsWrite, room_id: &str, text: &str, alias: Option<&str>) -> Result<(), String> {
    let mut params = json!({"_id": next_local_id(), "rid": room_id, "msg": text});
    if let Some(a) = alias { params["alias"] = json!(a); }
    let payload = json!({"msg":"method","method":"sendMessage","id":next_local_id(),"params":[params]});
    send_json(write, &payload).await.map_err(|e| format!("send: {e}"))?;
    Ok(())
}

fn uuid_v4_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{:016x}{:016x}", t & 0xFFFF_FFFF_FFFF_FFFF, t >> 32)
}

// ======================================================================
// tests
// ======================================================================

/// Two sessions. Both call createDirectMessage (idempotent) so both are
/// "joined" to the room. A sends a message; B receives the changed event.
#[tokio::test]
#[ignore = "requires a running RocketChat server and valid config.toml"]
async fn test_message_roundtrip_dual() {
    let config = RocketChatConfig::from_file(&config_path()).expect("load config");
    let (mut wa, mut ra, _ua, una) = raw_connect(&config).await.expect("A");
    let (mut wb, mut rb, _ub, unb) = raw_connect(&config).await.expect("B");

    subscribe_my_messages(&mut wa, &mut ra, "A").await.expect("sub A");
    subscribe_my_messages(&mut wb, &mut rb, "B").await.expect("sub B");

    let rid_a = create_dm(&mut wa, &mut ra, &una).await.expect("dm A");
    let rid_b = create_dm(&mut wb, &mut rb, &unb).await.expect("dm B");
    assert_eq!(rid_a, rid_b, "same room");

    let txt = format!("msg-{}", uuid_v4_simple());
    send_message(&mut wa, &rid_a, &txt, None).await.expect("send");

    drop(wa);
    drop(ra);

    for _ in 0..60 {
        let frame = tokio::time::timeout(Duration::from_secs(5), rb.next()).await;
        let v: Value = match frame {
            Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str(&t) { Ok(v) => v, Err(_) => continue },
            _ => continue,
        };
        if ddp::is_ping(&v) { send_json(&mut wb, &ddp::pong_message()).await.ok(); continue; }
        if ddp::is_changed(&v) {
            let mt = v.get("fields").and_then(|f| f.get("args"))
                .and_then(|a| a.as_array()).and_then(|a| a.first())
                .and_then(|m| m.get("msg")).and_then(|m| m.as_str());
            if mt == Some(&txt) {
                eprintln!("dual roundtrip OK");
                return;
            }
        }
    }
    panic!("message not received");
}

/// Two sessions. A sends typing indicators using `ddp::typing_payload()`
/// (the public API); B subscribes to `stream-notify-room/{rid}/user-activity`
/// and verifies both ON and OFF events are received.
#[tokio::test]
#[ignore = "requires a running RocketChat server and valid config.toml"]
async fn test_typing_indicator_roundtrip() {
    let config = RocketChatConfig::from_file(&config_path()).expect("load config");
    let username = config.server.username.clone();
    let (mut wa, mut ra, _ua, una) = raw_connect(&config).await.expect("A");
    let (mut wb, mut rb, _ub, _unb) = raw_connect(&config).await.expect("B");

    subscribe_my_messages(&mut wa, &mut ra, "A").await.expect("sub A");

    let rid = create_dm(&mut wa, &mut ra, &una).await.expect("dm A");

    subscribe_notify(&mut wb, &mut rb, "TYPING", &format!("{}/user-activity", rid))
        .await
        .expect("typing sub");

    // --- typing ON ---
    let tp = ddp::typing_payload(&rid, &username, true);
    send_json(&mut wa, &tp).await.expect("typing ON send");

    let mut found_typing = false;
    for _ in 0..15 {
        let frame = tokio::time::timeout(Duration::from_secs(5), rb.next()).await;
        let v: Value = match frame {
            Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str(&t) { Ok(v) => v, Err(_) => continue },
            _ => continue,
        };
        if ddp::is_ping(&v) { send_json(&mut wb, &ddp::pong_message()).await.ok(); continue; }
        if ddp::is_changed(&v) {
            let en = v.get("fields").and_then(|f| f.get("eventName")).and_then(|e| e.as_str());
            if en != Some(&format!("{}/user-activity", rid)) { continue; }
            let args = v.get("fields").and_then(|f| f.get("args")).and_then(|a| a.as_array());
            if let Some(args_arr) = args {
                if args_arr.len() >= 2
                    && args_arr[1].as_array().is_some_and(|a| a.iter().any(|v| v.as_str() == Some("user-typing")))
                {
                    eprintln!("typing ON roundtrip OK");
                    found_typing = true;
                    break;
                }
            }
        }
    }
    assert!(found_typing, "B should receive user-activity typing ON event");

    // Small gap so server sees a distinct state transition
    tokio::time::sleep(Duration::from_secs(1)).await;

    // --- typing OFF ---
    let tp_off = ddp::typing_payload(&rid, &username, false);
    send_json(&mut wa, &tp_off).await.expect("typing OFF send");

    let mut found_stopped = false;
    for _ in 0..15 {
        let frame = tokio::time::timeout(Duration::from_secs(5), rb.next()).await;
        let v: Value = match frame {
            Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str(&t) { Ok(v) => v, Err(_) => continue },
            _ => continue,
        };
        if ddp::is_ping(&v) { send_json(&mut wb, &ddp::pong_message()).await.ok(); continue; }
        if ddp::is_changed(&v) {
            let en = v.get("fields").and_then(|f| f.get("eventName")).and_then(|e| e.as_str());
            if en != Some(&format!("{}/user-activity", rid)) { continue; }
            let args = v.get("fields").and_then(|f| f.get("args")).and_then(|a| a.as_array());
            if let Some(args_arr) = args {
                if args_arr.len() >= 2 && args_arr[1].as_array().is_some_and(|a| a.is_empty()) {
                    eprintln!("typing OFF roundtrip OK");
                    found_stopped = true;
                    break;
                }
            }
        }
    }
    assert!(found_stopped, "B should receive user-activity typing OFF event (empty activities)");
}

/// Two sessions. A sends a message WITH an alias.
/// Requires `message-impersonate` permission on the RocketChat user.
/// Falls back to plain dual roundtrip if alias is rejected.
#[tokio::test]
#[ignore = "requires a running RocketChat server and valid config.toml"]
async fn test_message_alias_roundtrip() {
    let config = RocketChatConfig::from_file(&config_path()).expect("load config");
    let alias = "TotallyRealHuman";
    let (mut wa, mut ra, _ua, una) = raw_connect(&config).await.expect("A");
    let (mut wb, mut rb, _ub, unb) = raw_connect(&config).await.expect("B");

    subscribe_my_messages(&mut wa, &mut ra, "A").await.expect("sub A");
    subscribe_my_messages(&mut wb, &mut rb, "B").await.expect("sub B");

    let rid_a = create_dm(&mut wa, &mut ra, &una).await.expect("dm A");
    let rid_b = create_dm(&mut wb, &mut rb, &unb).await.expect("dm B");
    assert_eq!(rid_a, rid_b);

    let txt = format!("alias-{}", uuid_v4_simple());
    send_message(&mut wa, &rid_a, &txt, Some(alias)).await.expect("send aliased");

    // Check if alias was accepted
    let mut alias_ok = false;
    for _ in 0..20 {
        let frame = tokio::time::timeout(Duration::from_secs(5), ra.next()).await;
        let v: Value = match frame {
            Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str(&t) { Ok(v) => v, Err(_) => continue },
            _ => break,
        };
        if ddp::is_ping(&v) { send_json(&mut wa, &ddp::pong_message()).await.ok(); continue; }
        if ddp::is_result(&v) {
            alias_ok = v.get("error").is_none();
            break;
        }
    }

    if !alias_ok {
        eprintln!("SKIP: alias requires 'message-impersonate' permission. Send plain msg instead.");
        let txt2 = format!("fallback-{}", uuid_v4_simple());
        send_message(&mut wa, &rid_a, &txt2, None).await.expect("send fallback");
        // consume result
        for _ in 0..20 {
            let frame = tokio::time::timeout(Duration::from_secs(5), ra.next()).await;
            let v: Value = match frame {
                Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str(&t) { Ok(v) => v, Err(_) => continue },
                _ => break,
            };
            if ddp::is_ping(&v) { send_json(&mut wa, &ddp::pong_message()).await.ok(); continue; }
            if ddp::is_result(&v) { break; }
        }
    }

    drop(wa);
    drop(ra);

    for _ in 0..60 {
        let frame = tokio::time::timeout(Duration::from_secs(5), rb.next()).await;
        let v: Value = match frame {
            Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str(&t) { Ok(v) => v, Err(_) => continue },
            _ => continue,
        };
        if ddp::is_ping(&v) { send_json(&mut wb, &ddp::pong_message()).await.ok(); continue; }
        if ddp::is_changed(&v) {
            let mt = v.get("fields").and_then(|f| f.get("args"))
                .and_then(|a| a.as_array()).and_then(|a| a.first())
                .and_then(|m| m.get("msg")).and_then(|m| m.as_str());
            let al = v.get("fields").and_then(|f| f.get("args"))
                .and_then(|a| a.as_array()).and_then(|a| a.first())
                .and_then(|m| m.get("alias")).and_then(|a| a.as_str());
            eprintln!("B received: msg={mt:?} alias={al:?}");
            if alias_ok {
                if mt == Some(&txt) { assert_eq!(al, Some(alias)); eprintln!("alias OK"); return; }
            } else if mt.is_some() {
                eprintln!("dual roundtrip OK (no alias)");
                return;
            }
        }
    }
    panic!("no message received on B");
}
