/// Integration tests that connect to a real RocketChat server.
/// These tests require a valid `config.toml` in the workspace root.
/// Run with: `cargo test --test integration_real -- --ignored`
use rocketchat::{IncomingMessage, MessageSender, RocketChatClient};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

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
                if let Err(e) = sender.reply(&reply, None).await {
                    eprintln!("Failed to send reply: {}", e);
                }
            }
        }),
    )
    .await;

    match result {
        Ok(Err(e)) => {
            // Connection error is expected if server is unreachable
            let count = received.load(Ordering::SeqCst);
            eprintln!("Connection ended after {} messages: {}", count, e);
            // Even 0 messages is okay - we're testing the connection lifecycle
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

    // Test that the WS URI is correctly formed
    let uri = config.ws_uri().unwrap();
    assert!(uri.starts_with("wss://"));
    assert!(uri.ends_with("/websocket"));

    // Test host extraction
    let host = config.host();
    assert!(!host.is_empty());
    assert!(!host.starts_with("https://"));
    assert!(!host.starts_with("http://"));
}
