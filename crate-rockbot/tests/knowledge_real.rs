use std::env;

use rockbot::knowledge::{IndexEntry, KnowledgeCategory, KnowledgeIndex, KnowledgeManager, KnowledgePriority};
use webdav::WebDavClient;

fn get_webdav_client() -> Option<WebDavClient> {
    if let (Ok(url), Ok(username), Ok(password)) = (
        env::var("WEBDAV_URL"),
        env::var("WEBDAV_USERNAME"),
        env::var("WEBDAV_PASSWORD"),
    ) {
        let root = env::var("WEBDAV_ROOT").unwrap_or_else(|_| "rockbot-test".into());
        let full_url = format!("{url}/remote.php/dav/files/{username}/{root}");
        return WebDavClient::new(full_url, &username, &password).ok();
    }

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
            let candidates = [cwd.join("config.toml"), workspace_root.join("config.toml")];
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
    let root = wd.get("root").and_then(|r| r.as_str()).unwrap_or("rockbot-test");

    let full_url = format!("{url}/{root}");
    WebDavClient::new(full_url, &username, &password).ok()
}

/// Verify the full save → index read → retrieve cycle with the new
/// IndexEntry shape (filename + when_useful + tags, no updated_at).
#[tokio::test]
#[ignore]
async fn test_knowledge_save_and_read_index() {
    let Some(client) = get_webdav_client() else {
        eprintln!("Skipping: no WebDAV credentials");
        return;
    };

    let webdav_dir = "r-knowledge-real-test";
    let dir = KnowledgeManager::knowledge_dir(webdav_dir);
    let index_path = KnowledgeManager::index_path(webdav_dir);

    // Ensure clean state
    client.delete(&index_path).await.ok();

    // Save a knowledge entry
    KnowledgeManager::save_entry(
        &client,
        webdav_dir,
        &KnowledgeCategory::Note,
        "Real test entry topic",
        "## Content\n\nThis is a real test entry body.",
        "When you need to test knowledge persistence",
        &["testing".into(), "real".into(), "integration".into()],
        &KnowledgePriority::P1,
    )
    .await
    .expect("save_entry failed");

    // Read index.json back and verify shape
    let index = KnowledgeManager::load_index(&client, webdav_dir)
        .await
        .expect("load_index failed");

    eprintln!("Index version: {}", index.version);
    eprintln!("Index room_id: {}", index.room_id);
    eprintln!("Index entries ({}) :", index.entries.len());
    for entry in &index.entries {
        eprintln!(
            "  filename={} when_useful={} tags={:?}",
            entry.filename, entry.when_useful, entry.tags
        );
    }

    assert_eq!(index.entries.len(), 1, "should have one entry");
    let entry = &index.entries[0];

    // Check filename shape
    assert!(entry.filename.ends_with(".md"), "filename should end with .md");
    assert!(entry.filename.starts_with("note_"), "should be a note category entry");
    assert!(
        entry.filename.contains("real_test_entry"),
        "filename should contain slugified topic"
    );

    // Check new fields
    assert!(
        !entry.when_useful.is_empty(),
        "when_useful should be populated in index"
    );
    assert_eq!(
        entry.tags,
        vec!["testing".to_string(), "real".to_string(), "integration".to_string()],
        "tags should be populated in index"
    );

    // Verify index.json is valid JSON on disk
    let raw = client
        .read_file_to_string(&index_path)
        .await
        .expect("should be able to read index.json raw");
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).expect("index.json should be valid JSON");
    let raw_entry = &parsed["entries"][0];
    assert!(raw_entry.get("filename").is_some(), "raw JSON must have filename");
    assert!(raw_entry.get("when_useful").is_some(), "raw JSON must have when_useful");
    assert!(raw_entry.get("tags").is_some(), "raw JSON must have tags");

    // recall_entry should find it
    let result = KnowledgeManager::recall_entry(&client, webdav_dir, "test")
        .await
        .expect("recall_entry failed");
    assert!(result.is_some(), "should find entry by query");
    let result = result.unwrap();
    assert!(result.contains("Real test entry topic"), "result should contain title");
    assert!(result.contains("real test entry body"), "result should contain content");

    // Clean up
    client.delete(&index_path).await.ok();
    let md_path = format!("{}{}", dir, entry.filename);
    client.delete(&md_path).await.ok();
    client.delete(&dir).await.ok();
}

/// Verify match_relevant() uses when_useful and tags for scoring.
#[tokio::test]
#[ignore]
async fn test_knowledge_match_relevant_with_new_fields() {
    let Some(client) = get_webdav_client() else {
        eprintln!("Skipping: no WebDAV credentials");
        return;
    };

    let webdav_dir = "r-match-real-test";
    let dir = KnowledgeManager::knowledge_dir(webdav_dir);
    let index_path = KnowledgeManager::index_path(webdav_dir);

    client.delete(&index_path).await.ok();

    // Save two entries with distinct when_useful and tags
    KnowledgeManager::save_entry(
        &client,
        webdav_dir,
        &KnowledgeCategory::Skill,
        "Build Cargo Project",
        "Run `cargo build --release` to build.",
        "When asked about building Rust projects or compiling code",
        &["rust".into(), "build".into()],
        &KnowledgePriority::P1,
    )
    .await
    .expect("save entry 1");

    KnowledgeManager::save_entry(
        &client,
        webdav_dir,
        &KnowledgeCategory::Note,
        "Office Phone Number",
        "Call 555-0199 for support.",
        "When asked about contact info or phone numbers",
        &["contact".into(), "phone".into()],
        &KnowledgePriority::P1,
    )
    .await
    .expect("save entry 2");

    let index = KnowledgeManager::load_index(&client, webdav_dir)
        .await
        .expect("load_index");

    // Match by tag keyword
    let matches =
        KnowledgeManager::match_relevant(&index, &["what is the phone number for the office?"]);
    eprintln!("Phone match results:");
    for m in &matches {
        eprintln!("  {}  when_useful={}  tags={:?}", m.filename, m.when_useful, m.tags);
    }
    assert!(
        matches.iter().any(|e| e.filename.contains("phone")),
        "should find phone entry by tag 'phone' or when_useful"
    );

    // Match by when_useful keyword
    let matches = KnowledgeManager::match_relevant(&index, &["how do I compile this Rust code?"]);
    eprintln!("Rust match results:");
    for m in &matches {
        eprintln!("  {}  when_useful={}  tags={:?}", m.filename, m.when_useful, m.tags);
    }
    assert!(
        matches.iter().any(|e| e.filename.contains("build")),
        "should find build entry by tag or when_useful overlap"
    );

    // Match by title keyword
    let matches = KnowledgeManager::match_relevant(&index, &["how do I build the cargo thing?"]);
    assert!(
        matches.iter().any(|e| e.filename.contains("build")),
        "should find build entry by title keyword overlap"
    );

    // Clean up
    let index_for_cleanup = KnowledgeManager::load_index(&client, webdav_dir)
        .await
        .unwrap_or_else(|_| KnowledgeIndex {
            version: String::new(),
            room_id: webdav_dir.to_string(),
            entries: vec![],
            updated: String::new(),
        });
    for entry in &index_for_cleanup.entries {
        let md_path = format!("{}{}", dir, entry.filename);
        client.delete(&md_path).await.ok();
    }
    client.delete(&index_path).await.ok();
    client.delete(&dir).await.ok();
}

/// Verify the KnowledgeManager module is accessible from tests
/// (compile-time check that IndexEntry and KnowledgeManager are public).
#[test]
fn test_knowledge_module_is_public() {
    // Just ensure IndexEntry is accessible — if it compiles, it's public
    let _entry = IndexEntry {
        filename: "test.md".into(),
        when_useful: "test situation".into(),
        tags: vec!["test".into()],
    };
    assert_eq!(_entry.display_title(), "test");
}
