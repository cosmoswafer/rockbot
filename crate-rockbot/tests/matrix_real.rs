/// Integration probe: connect to a real Matrix server and list joined rooms.
/// Requires a valid `config.toml` with `[matrix.server]` in the workspace root.
///
/// Run with:
///   RUST_LOG=debug cargo test -p rockbot --test matrix_real -- --ignored --nocapture
use matrix_sdk::config::SyncSettings;
use matrix_sdk::Client;
use std::time::Duration;

fn load_matrix_config() -> (String, String, String, String) {
    let path = if std::path::Path::new("config.toml").exists() {
        "config.toml"
    } else if std::path::Path::new("../../config.toml").exists() {
        "../../config.toml"
    } else if std::path::Path::new("../config.toml").exists() {
        "../config.toml"
    } else {
        panic!("config.toml not found");
    };
    let content = std::fs::read_to_string(path).expect("read config.toml");
    let table: toml::Table = toml::from_str(&content).expect("parse config.toml");

    let matrix = table.get("matrix")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("server"))
        .and_then(|v| v.as_table())
        .expect("[matrix.server] section required");

    let homeserver = matrix.get("homeserver").and_then(|v| v.as_str()).expect("homeserver").to_string();
    let user_id = matrix.get("user_id").and_then(|v| v.as_str()).expect("user_id").to_string();
    let password = matrix.get("password").and_then(|v| v.as_str()).expect("password").to_string();
    let device_id = matrix.get("device_id").and_then(|v| v.as_str()).unwrap_or("PROBE").to_string();

    (homeserver, user_id, password, device_id)
}

#[tokio::test]
#[ignore]
async fn probe_matrix_login_and_rooms() {
    let (homeserver, user_id, password, device_id) = load_matrix_config();
    println!("\n=== Matrix Integration Probe ===");
    println!("Homeserver: {}", homeserver);
    println!("User ID:    {}", user_id);
    println!("Device ID:  {}", device_id);

    let client = Client::builder()
        .homeserver_url(&homeserver)
        .build()
        .await
        .expect("build Matrix client");

    client
        .matrix_auth()
        .login_username(&user_id, &password)
        .device_id(&device_id)
        .send()
        .await
        .expect("Matrix login");

    let resolved_user_id = client.user_id().map(|u| u.to_string()).unwrap_or_default();
    let resolved_device_id = client.device_id().map(|d| d.to_string()).unwrap_or_default();
    println!("\n--- Login Result ---");
    println!("Resolved user_id:   {}", resolved_user_id);
    println!("Resolved device_id: {}", resolved_device_id);

    println!("\n--- Syncing (one pass, 5s timeout) ---");
    client
        .sync_once(SyncSettings::default().timeout(Duration::from_secs(5)))
        .await
        .expect("sync_once");

    let joined_rooms = client.joined_rooms();
    println!("\n--- Joined Rooms ({}) ---", joined_rooms.len());

    for room in &joined_rooms {
        let room_id = room.room_id().to_string();
        let display_name = room.display_name().await.ok().map(|dn| match dn {
            matrix_sdk::RoomDisplayName::Named(name) => name.to_string(),
            matrix_sdk::RoomDisplayName::Calculated(name) => name.to_string(),
            matrix_sdk::RoomDisplayName::Empty => "(empty)".to_string(),
            _ => "(other)".to_string(),
        }).unwrap_or_else(|| "(error)".to_string());
        let member_count = room.active_members_count();
        let canonical_alias = room.canonical_alias()
            .map(|a| a.alias().to_string())
            .unwrap_or_default();
        let topic = room.topic().unwrap_or_default();
        println!("  {} | {} | members={} | alias={} | topic={}", room_id, display_name, member_count, canonical_alias, topic);
    }

    println!("\n=== Probe Complete ===");
}
