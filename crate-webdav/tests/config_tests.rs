use webdav::WebDavConfig;

#[test]
fn test_deserialize_minimal() {
    let toml = r#"
url = "https://cloud.example.com/remote.php/dav/files/botuser"
username = "botuser"
password = "app-secret"
root = "rockbot"
"#;
    let cfg = WebDavConfig::from_toml(toml).expect("should parse");
    assert_eq!(
        cfg.url,
        "https://cloud.example.com/remote.php/dav/files/botuser"
    );
    assert_eq!(cfg.username, "botuser");
    assert_eq!(cfg.password, "app-secret");
    assert_eq!(cfg.root, "rockbot");
}

#[test]
fn test_deserialize_url_with_trailing_slash() {
    let toml = r#"
url = "https://cloud.example.com/remote.php/dav/files/botuser/"
username = "botuser"
password = "app-secret"
root = "rockbot"
"#;
    let cfg = WebDavConfig::from_toml(toml).expect("should parse");
    let client = cfg.create_client();
    assert!(client.is_ok());
}

#[test]
fn test_deserialize_root_with_slashes() {
    let toml = r#"
url = "https://cloud.example.com/remote.php/dav/files/botuser"
username = "botuser"
password = "app-secret"
root = "/rockbot/"
"#;
    let cfg = WebDavConfig::from_toml(toml).expect("should parse");
    let client = cfg.create_client();
    assert!(client.is_ok());
}

#[test]
fn test_into_client() {
    let toml = r#"
url = "https://cloud.example.com/remote.php/dav/files/botuser"
username = "botuser"
password = "app-secret"
root = "rockbot"
"#;
    let cfg = WebDavConfig::from_toml(toml).expect("should parse");
    let client = cfg.into_client();
    assert!(client.is_ok());
}

#[test]
fn test_base_url_construction() {
    let toml = r#"
url = "https://cloud.example.com/remote.php/dav/files/botuser"
username = "botuser"
password = "secret"
root = "rockbot"
"#;
    let cfg = WebDavConfig::from_toml(toml).expect("should parse");
    assert!(cfg.create_client().is_ok());
}

#[test]
fn test_missing_field_fails() {
    let toml = r#"
url = "https://cloud.example.com/remote.php/dav/files/botuser"
username = "botuser"
"#;
    let result = WebDavConfig::from_toml(toml);
    assert!(result.is_err());
}
