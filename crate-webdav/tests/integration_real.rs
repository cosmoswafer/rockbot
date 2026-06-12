use std::env;
use webdav::{WebDavClient, WebDavEntry, WebDavPath};

/// Tries to get a client from WEBDAV_* env vars first, then falls back
/// to reading `config.toml` from the workspace root (for local testing).
fn get_client() -> Option<WebDavClient> {
    // 1) try env vars
    if let (Ok(url), Ok(username), Ok(password)) = (
        env::var("WEBDAV_URL"),
        env::var("WEBDAV_USERNAME"),
        env::var("WEBDAV_PASSWORD"),
    ) {
        let root = env::var("WEBDAV_ROOT").unwrap_or_else(|_| "rockbot-test".into());
        let full_url = format!("{url}/remote.php/dav/files/{username}/{root}");
        return WebDavClient::new(full_url, &username, &password).ok();
    }

    // 2) try CONFIG_FILE env var, then config.toml in workspace/cwd
    let config_path = {
        if let Ok(p) = env::var("CONFIG_FILE") {
            let pb = std::path::PathBuf::from(&p);
            if pb.exists() {
                Some(pb)
            } else {
                None
            }
        } else {
            let cwd = std::env::current_dir().ok()?;
            let workspace_root = cwd.parent().unwrap_or(&cwd);
            let candidates = [
                cwd.join("config.toml"),
                workspace_root.join("config.toml"),
            ];
            candidates.into_iter().find(|p| p.exists())
        }
    };
    let config_path = config_path?;

    let config_str = std::fs::read_to_string(&config_path).ok()?;
    let config: toml::Table = toml::from_str(&config_str).ok()?;
    let wd = config.get("webdav")?;

    let url = wd.get("url")?.as_str()?;
    let username = wd.get("username")?.as_str()?;
    let password = wd.get("password")?.as_str()?;
    let root = wd
        .get("root")
        .and_then(|r| r.as_str())
        .unwrap_or("rockbot-test");

    let full_url = format!("{url}/{root}");
    WebDavClient::new(full_url, username, password).ok()
}

/// Generate a unique test directory name to avoid 423 FileLocked errors
/// from previous orphaned test runs.
fn unique_dir(name: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{}-{}", name, ts)
}

#[tokio::test]
#[ignore]
async fn test_real_ensure_directory_and_list() {
    let client = match get_client() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: no credentials available (env vars or config.toml)");
            return;
        }
    };

    let test_dir = format!("/test-run/{}/", unique_dir("real-test-dir"));

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
            eprintln!("Skipping test: no credentials available (env vars or config.toml)");
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
            eprintln!("Skipping test: no credentials available (env vars or config.toml)");
            return;
        }
    };

    let dir = format!("/test-run/{}/", unique_dir("auto-mkcol-test"));
    let file_path = format!("{dir}deep/nested/file.txt");

    match client
        .write_file_auto_mkcol(&file_path, "auto-created dirs")
        .await
    {
        Ok(()) => {
            let content = client
                .read_file_to_string(&file_path)
                .await
                .expect("Failed to read file");
            assert_eq!(content, "auto-created dirs");
        }
        Err(webdav::WebDavError::NotFound(_)) => {
            eprintln!("AutoMkcol not supported by this server (needs NextCloud 32+) — skipping");
        }
        Err(e) => panic!("Unexpected error: {e}"),
    }

    client.delete(&dir).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_real_exists() {
    let client = match get_client() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: no credentials available (env vars or config.toml)");
            return;
        }
    };

    let path = WebDavPath::new("test-run");
    let dir = path.room_dir("exists-test");

    assert!(!client.exists(&dir).await.unwrap_or(true));

    client.ensure_directory_all(&dir).await.ok();

    let exists = client
        .exists(&dir)
        .await
        .expect("Failed to check existence");
    assert!(exists, "Directory should exist after creation");

    client.delete(&dir).await.ok();
}

// ── Comprehensive PROPFIND directory listing test ───────────────────────────
//
// This test exercises the full WebDAV PROPFIND flow against a real NextCloud
// server.  It creates a test directory with a mix of files and subdirs, then
// verifies every field returned by `list_directory`:
//   - name / href / is_dir / size / modified
//
// Official NextCloud WebDAV docs reference:
//   https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/basic.html

/// Helper: ensure the test-run root exists so we can clean up at end.
async fn ensure_test_run_root(client: &WebDavClient) -> String {
    let path = WebDavPath::new("test-run");
    let root = path.room_dir("");
    client.ensure_directory_all(&root).await.ok();
    root
}

async fn test_list_entries(client: &WebDavClient, dir: &str) -> Vec<WebDavEntry> {
    client
        .list_directory(dir)
        .await
        .expect("PROPFIND list_directory failed")
}

#[tokio::test]
#[ignore]
async fn test_real_list_directory_empty() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: no credentials");
        return;
    };

    let root = ensure_test_run_root(&client).await;
    let dir = format!("{root}list-empty/");
    client.ensure_directory_all(&dir).await.expect("mkdir");
    eprintln!("Test dir: {}", dir);

    let entries = test_list_entries(&client, &dir).await;
    eprintln!("Entries: {entries:#?}");

    // The PROPFIND response MUST include a self-reference entry for the
    // directory itself, even when empty.
    assert!(
        entries.iter().any(|e| e.is_dir && e.href.contains("list-empty")),
        "empty directory must have a self-reference entry"
    );

    // Clean up
    client.delete(&dir).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_real_list_directory_with_files() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: no credentials");
        return;
    };

    let root = ensure_test_run_root(&client).await;
    let dir = format!("{root}list-files/");
    client.ensure_directory_all(&dir).await.expect("mkdir");
    eprintln!("Test dir: {}", dir);

    // Write a few files
    client
        .write_file(&format!("{dir}alpha.txt"), "alpha content here\n")
        .await
        .expect("write alpha");
    client
        .write_file(&format!("{dir}beta.md"), "# Beta\n\nmarkdown file")
        .await
        .expect("write beta");
    client
        .write_file(&format!("{dir}gamma.log"), "INFO  ok\nERROR fail\n")
        .await
        .expect("write gamma");

    // Create a subdirectory
    let sub = format!("{dir}subdir/");
    client.ensure_directory_all(&sub).await.expect("mkdir sub");

    let entries = test_list_entries(&client, &dir).await;

    // Dump entries for manual inspection
    for e in &entries {
        eprintln!("  {}  {:>8}  {}  {}", if e.is_dir { "DIR" } else { "   " }, format_size(e.size), e.modified, e.name);
    }

    // ── assertions ──────────────────────────────────────────────────────

    // Self-reference (directory itself) must be present
    assert!(
        entries.iter().any(|e| e.is_dir && e.href.contains("list-files") && e.name == "list-files"),
        "must have self-reference entry"
    );

    // alpha.txt – file, has content length
    let alpha = entries
        .iter()
        .find(|e| e.name == "alpha.txt")
        .expect("alpha.txt not found");
    assert!(!alpha.is_dir, "alpha.txt should be a file");
    assert!(alpha.size > 0, "alpha.txt should have non-zero size");
    assert!(!alpha.modified.is_empty(), "alpha.txt should have modified timestamp");

    // beta.md
    let beta = entries
        .iter()
        .find(|e| e.name == "beta.md")
        .expect("beta.md not found");
    assert!(!beta.is_dir);
    assert!(beta.size > 0);

    // gamma.log
    let gamma = entries
        .iter()
        .find(|e| e.name == "gamma.log")
        .expect("gamma.log not found");
    assert!(!gamma.is_dir);
    assert!(gamma.size > 0);

    // subdir
    let subdir = entries
        .iter()
        .find(|e| e.name == "subdir")
        .expect("subdir not found");
    assert!(subdir.is_dir, "subdir should be a directory");
    assert_eq!(subdir.size, 0, "directories have size 0 (use oc:size for dir sizes)");

    // ── clean up ────────────────────────────────────────────────────────
    client.delete(&format!("{dir}alpha.txt")).await.ok();
    client.delete(&format!("{dir}beta.md")).await.ok();
    client.delete(&format!("{dir}gamma.log")).await.ok();
    client.delete(&sub).await.ok();
    client.delete(&dir).await.ok();
}

#[tokio::test]
#[ignore]
async fn test_real_list_handles_special_chars() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: no credentials");
        return;
    };

    let root = ensure_test_run_root(&client).await;
    let dir = format!("{root}list-special/");
    client.ensure_directory_all(&dir).await.expect("mkdir");

    // File with spaces, Unicode, and dots
    let filename = "résumé (2026) v2.0.txt";
    client
        .write_file(&format!("{dir}{filename}"), "contenu")
        .await
        .expect("write special");

    let entries = test_list_entries(&client, &dir).await;
    for e in &entries {
        eprintln!("  {}  {:>8}  {}  {}", if e.is_dir { "DIR" } else { "   " }, format_size(e.size), e.modified, e.name);
    }

    let found = entries.iter().find(|e| e.name == filename);
    assert!(found.is_some(), "file with special chars not found in listing");
    let f = found.unwrap();
    assert!(!f.is_dir);
    assert!(f.size > 0);

    // Clean up
    client.delete(&format!("{dir}{filename}")).await.ok();
    client.delete(&dir).await.ok();
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "-".to_string();
    }
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
