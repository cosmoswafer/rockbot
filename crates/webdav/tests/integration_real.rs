use std::env;
use webdav::{WebDavClient, WebDavPath};

fn get_client() -> Option<WebDavClient> {
    let url = env::var("WEBDAV_URL").ok()?;
    let username = env::var("WEBDAV_USERNAME").ok()?;
    let password = env::var("WEBDAV_PASSWORD").ok()?;
    let root = env::var("WEBDAV_ROOT").unwrap_or_else(|_| "rockbot-test".into());

    let full_url = format!("{url}/remote.php/dav/files/{username}/{root}");
    WebDavClient::new(full_url, &username, &password).ok()
}

#[tokio::test]
#[ignore]
async fn test_real_ensure_directory_and_list() {
    let client = match get_client() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: no WEBDAV_* env vars set");
            return;
        }
    };

    let path = WebDavPath::new("test-run");
    let test_dir = path.room_dir("real-test-dir");

    client
        .ensure_directory_all(&test_dir)
        .await
        .expect("Failed to create test directory");

    let entries = client
        .list_directory(&test_dir)
        .await
        .expect("Failed to list directory");

    assert!(
        entries.iter().any(|e| e.href.contains("real-test-dir")),
        "Should find the self-reference entry"
    );

    client.delete(&test_dir).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_real_write_and_read_file() {
    let client = match get_client() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: no WEBDAV_* env vars set");
            return;
        }
    };

    let path = WebDavPath::new("test-run");
    let dir = path.room_dir("real-io-test");
    client.ensure_directory_all(&dir).await.ok();

    let file_path = format!("{dir}hello.txt");

    client
        .write_file(&file_path, "Hello WebDAV!")
        .await
        .expect("Failed to write file");

    let content = client
        .read_file_to_string(&file_path)
        .await
        .expect("Failed to read file");

    assert_eq!(content, "Hello WebDAV!");

    client.delete(&file_path).await.ok();
    client.delete(&dir).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_real_write_file_auto_mkcol() {
    let client = match get_client() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: no WEBDAV_* env vars set");
            return;
        }
    };

    let path = WebDavPath::new("test-run");
    let dir = path.room_dir("auto-mkcol-test");
    let file_path = format!("{dir}deep/nested/file.txt");

    client
        .write_file_auto_mkcol(&file_path, "auto-created dirs")
        .await
        .expect("Failed to write with auto mkcol");

    let content = client
        .read_file_to_string(&file_path)
        .await
        .expect("Failed to read file");

    assert_eq!(content, "auto-created dirs");

    client.delete(&dir).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_real_exists() {
    let client = match get_client() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: no WEBDAV_* env vars set");
            return;
        }
    };

    let path = WebDavPath::new("test-run");
    let dir = path.room_dir("exists-test");

    assert!(!client.exists(&dir).await.unwrap_or(true));

    client.ensure_directory_all(&dir).await.ok();

    let exists = client.exists(&dir).await.expect("Failed to check existence");
    assert!(exists, "Directory should exist after creation");

    client.delete(&dir).await.ok();
}
