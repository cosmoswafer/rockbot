use webdav::{WebDavClient, WebDavPath};

#[test]
fn test_client_new() {
    let client = WebDavClient::new(
        "https://cloud.example.com/remote.php/dav/files/user",
        "user",
        "app-password",
    );
    assert!(client.is_ok());
}

#[test]
fn test_client_new_with_trailing_slash() {
    let client = WebDavClient::new(
        "https://cloud.example.com/remote.php/dav/files/user/",
        "user",
        "pass",
    );
    assert!(client.is_ok());
}

#[test]
fn test_webdav_path_room_dir() {
    let path = WebDavPath::new("rockbot");
    assert_eq!(path.room_dir("general"), "/rockbot/general/");
}

#[test]
fn test_webdav_path_memory_dir() {
    let path = WebDavPath::new("rockbot");
    assert_eq!(path.memory_dir("dm-alice"), "/rockbot/dm-alice/memory/");
}

#[test]
fn test_webdav_path_image_path() {
    let path = WebDavPath::new("rockbot");
    assert_eq!(
        path.image_path("general", "photo.png").unwrap(),
        "/rockbot/general/images/photo.png"
    );
}

#[test]
fn test_webdav_path_root_trim() {
    let path = WebDavPath::new("/rockbot/");
    assert_eq!(path.root, "rockbot");
    assert_eq!(path.room_dir("ch"), "/rockbot/ch/");
}

#[test]
fn test_webdav_path_image_dir() {
    let path = WebDavPath::new("botdata");
    assert_eq!(path.image_dir("general"), "/botdata/general/images/");
}

#[test]
fn test_webdav_path_workspace_dir() {
    let path = WebDavPath::new("botdata");
    assert_eq!(path.workspace_dir("general"), "/botdata/general/workspace/");
}

#[test]
fn test_webdav_path_room_path() {
    let path = WebDavPath::new("rockbot");
    assert_eq!(
        path.room_path("general", "notes.txt").unwrap(),
        "/rockbot/general/notes.txt"
    );
    assert_eq!(
        path.room_path("dm-alice", "sub/notes.txt").unwrap(),
        "/rockbot/dm-alice/sub/notes.txt"
    );
}

#[test]
fn test_webdav_path_config_backup_path() {
    let path = WebDavPath::new("rockbot");
    assert_eq!(
        path.config_backup_path("2026-06-01_config.toml"),
        "/rockbot/config/2026-06-01_config.toml"
    );
}
