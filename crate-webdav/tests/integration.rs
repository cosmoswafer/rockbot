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

const OCS_SHARE_XML: &str = r#"<?xml version="1.0"?>
<ocs>
  <meta>
    <status>ok</status>
    <statuscode>100</statuscode>
  </meta>
  <data>
    <id>42</id>
    <url>https://nc.example.com/s/iPNxaew4YLjeGzG</url>
  </data>
</ocs>"#;

fn ocs_share_matchers() -> wiremock::MockBuilder {
    wiremock::Mock::given(wiremock::matchers::method("POST")).and(
        wiremock::matchers::path("/ocs/v2.php/apps/files_sharing/api/v1/shares"),
    )
}

// DFD tools/image-gen: OCS share creation happy path — returned share_url must
// carry the /preview suffix (inline rendering, real MIME type), not /download
// (303 redirect + Content-Disposition: attachment). See issue #97.
#[tokio::test]
async fn test_create_nextcloud_share_link_appends_preview_suffix() {
    let server = wiremock::MockServer::start().await;
    ocs_share_matchers()
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(OCS_SHARE_XML))
        .expect(1)
        .mount(&server)
        .await;

    let client =
        WebDavClient::new(format!("{}/remote.php/dav/files/user", server.uri()), "user", "pass")
            .unwrap();
    let share = client
        .create_nextcloud_share_link("/rockbot/general/images/pic.png")
        .await
        .expect("share link should be created from OCS <url>");

    // The OCS <url> is authoritative (public share host), suffix appended locally
    assert_eq!(share, "https://nc.example.com/s/iPNxaew4YLjeGzG/preview");
    assert!(!share.ends_with("/download"));
    server.verify().await;
}

// DFD tools/image-gen fallback: no <url> element in OCS response → None,
// callers fall back to DDP attachment path.
#[tokio::test]
async fn test_create_nextcloud_share_link_none_without_url_element() {
    let server = wiremock::MockServer::start().await;
    ocs_share_matchers()
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string("<ocs><meta><status>failure</status></meta><data/></ocs>"),
        )
        .mount(&server)
        .await;

    let client =
        WebDavClient::new(format!("{}/remote.php/dav/files/user", server.uri()), "user", "pass")
            .unwrap();
    assert!(client.create_nextcloud_share_link("/rockbot/general/images/pic.png").await.is_none());
}
