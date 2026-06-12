use std::collections::HashMap;
use std::path::PathBuf;

use rockbot::image_cache::ImageCache;
use rockbot::tools::ImageGenTool;
use rockbot::validated::{ConfigUrl, ProviderName};
use rockbot::ProviderConfig;
use rockbot::Tool;
use webdav::WebDavClient;

/// Find the workspace root by walking up from CARGO_MANIFEST_DIR.
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().to_path_buf()
}

/// Load the config.toml as a raw toml::Value.
fn load_config() -> Option<toml::Value> {
    let candidates = [
        workspace_root().join("config.toml"),
        PathBuf::from("config.toml"),
    ];
    for path in &candidates {
        if path.exists() {
            let contents = std::fs::read_to_string(path).ok()?;
            return toml::from_str(&contents).ok();
        }
    }
    None
}

/// Extract an image_provider entry from config.toml by name.
fn load_image_provider(name: &str) -> Option<ProviderConfig> {
    let config = load_config()?;
    let providers = config.get("image_providers")?.as_array()?;

    for provider in providers {
        if provider.get("name")?.as_str()? == name {
            let base_url = provider
                .get("base_url")
                .and_then(|v| v.as_str())
                .unwrap_or(if name == "fal" {
                    "https://queue.fal.run"
                } else {
                    "https://openrouter.ai/api/v1"
                });

            let mut models = HashMap::new();
            if let Some(models_table) = provider.get("models").and_then(|v| v.as_table()) {
                for (k, v) in models_table {
                    if let Some(val) = v.as_str() {
                        models.insert(k.clone(), val.to_string());
                    }
                }
            }

            return Some(ProviderConfig {
                name: ProviderName::try_new(name.to_string()).unwrap(),
                api_key: provider.get("api_key")?.as_str()?.into(),
                base_url: ConfigUrl::try_new(base_url.to_string()).unwrap(),
                basecf_url: None,
                chat_path: None,
                draw_path: None,
                models,
            });
        }
    }
    None
}

/// Load WebDAV config from config.toml.
fn load_webdav_config() -> Option<WebDavClient> {
    // Try env vars first
    if let (Ok(url), Ok(username), Ok(password)) = (
        std::env::var("WEBDAV_URL"),
        std::env::var("WEBDAV_USERNAME"),
        std::env::var("WEBDAV_PASSWORD"),
    ) {
        let root = std::env::var("WEBDAV_ROOT").unwrap_or_else(|_| "rockbot-test".into());
        let full_url = format!("{url}/remote.php/dav/files/{username}/{root}");
        return WebDavClient::new(full_url, &username, &password).ok();
    }

    let config = load_config()?;
    let wd = config.get("webdav")?;
    let url = wd.get("url")?.as_str()?;
    let username = wd.get("username")?.as_str()?;
    let password = wd.get("password")?.as_str()?;
    let root = wd.get("root").and_then(|r| r.as_str()).unwrap_or("rockbot");

    let full_url = format!("{url}/{root}");
    WebDavClient::new(full_url, username, password).ok()
}

// ---------------------------------------------------------------------------
// Real integration tests
// ---------------------------------------------------------------------------

/// Verify the full image_gen tool pipeline against a live image provider
/// and WebDAV server: prompt → provider.generate_image() → upload to WebDAV
/// → create NextCloud share link → store in ImageCache → return result.
///
/// Verifies the ImageGenResult data shape from image-gen.md §3:
/// `{"ok": true, "webdav_path": "...", "image_key": "..."}`
#[ignore]
#[tokio::test]
async fn test_image_gen_real_text_to_image() {
    // 1. Load image provider (fal first, then openrouter)
    let provider_cfg = match load_image_provider("fal") {
        Some(cfg) => {
            eprintln!("Using image provider: fal");
            cfg
        }
        None => {
            eprintln!("Skipping: no image_provider config (fal) found in config.toml");
            return;
        }
    };

    let model = provider_cfg
        .models
        .get("gptimage_text")
        .cloned()
        .unwrap_or_else(|| "fal-ai/flux-pro/v1.1-ultra".into());
    eprintln!("Model: {}", model);

    let provider = match rockbot::FalAiProvider::new(&provider_cfg, &model) {
        Ok(p) => Box::new(p),
        Err(e) => {
            eprintln!("Skipping: failed to create provider: {e}");
            return;
        }
    };
    eprintln!("Provider created: {}", provider.provider_name());

    // 2. Load WebDAV config
    let webdav = match load_webdav_config() {
        Some(client) => {
            eprintln!("WebDAV connected");
            client
        }
        None => {
            eprintln!("Skipping: no WebDAV config found (config.toml [webdav] or WEBDAV_* env vars)");
            return;
        }
    };

    // 3. Create ImageCache
    let image_cache = std::sync::Arc::new(ImageCache::new());

    // 4. Create ImageGenTool
    let tool = ImageGenTool::new(
        provider,
        "standard".into(), // default_quality
        "png".into(),      // default_output_format
        1,                 // default_num_images
        "square_1_1".into(), // default_image_size
        "2K".into(),       // default_image_size_tier
        webdav.clone(),
        image_cache.clone(),
    );
    eprintln!("ImageGenTool created: {}", tool.name());

    // 5. Execute the tool with a text-to-image prompt
    let args = serde_json::json!({
        "prompt": "A simple geometric test pattern: a blue square on a white background, minimal, clean",
        "room_id": "test-room-image-gen-real",
        "webdav_dir": "test-image-gen-real",
        "image_cache_key": "test-call-id-001",
    });
    eprintln!("Executing image_gen with args: {}", args);

    let t_start = std::time::Instant::now();
    let result = tool.execute(&args.to_string()).await;

    match result {
        Ok(json_str) => {
            let elapsed = t_start.elapsed();
            eprintln!("image_gen completed in {elapsed:.2?}");
            eprintln!("Result JSON: {}", json_str);

            // 6. Validate DFD ImageGenResult shape
            let parsed: serde_json::Value =
                serde_json::from_str(&json_str).expect("Result should be valid JSON");
            assert_eq!(parsed["ok"], true, "ImageGenResult.ok should be true");
            assert!(
                parsed["webdav_path"].is_string(),
                "ImageGenResult.webdav_path should be a string: got {:?}",
                parsed["webdav_path"]
            );
            assert!(
                parsed["image_key"].is_string(),
                "ImageGenResult.image_key should be a string: got {:?}",
                parsed["image_key"]
            );

            let webdav_path = parsed["webdav_path"].as_str().unwrap();
            eprintln!("  webdav_path: {}", webdav_path);
            eprintln!("  image_key:   {}", parsed["image_key"].as_str().unwrap());

            // Verify share_url presence (may or may not be present)
            if let Some(share_url) = parsed.get("share_url").and_then(|v| v.as_str()) {
                eprintln!("  share_url:   {}", share_url);
                assert!(
                    share_url.starts_with("https://"),
                    "share_url should be an HTTPS URL: got {}",
                    share_url
                );
            } else {
                eprintln!("  share_url:   (none — NextCloud sharing may not be configured)");
            }

            // 7. Verify the image was stored in ImageCache
            let cached = image_cache.take("test-call-id-001");
            assert!(cached.is_some(), "Image should be in ImageCache for key test-call-id-001");
            let cached = cached.unwrap();
            assert_eq!(cached.webdav_path.as_str(), webdav_path);
            assert!(!cached.image_bytes.is_empty(), "Cached image bytes should not be empty");
            eprintln!("  cached bytes: {}", cached.image_bytes.len());
            eprintln!("  cached mime:  {}", cached.mime_type.as_str());

            // 8. Verify the image exists on WebDAV
            let exists = webdav
                .exists(webdav_path)
                .await
                .unwrap_or(false);
            assert!(exists, "Generated image should exist on WebDAV at {}", webdav_path);
            eprintln!("  WebDAV exists: true");

            // 9. Clean up — delete the test image from WebDAV
            if exists {
                let _ = webdav.delete(webdav_path).await;
                eprintln!("  Cleaned up WebDAV file");
            }

            // 10. Data collection for DFD verification
            eprintln!("\n--- Data Shape Collection for DFD verification ---");
            eprintln!("ImageGenResult format:                {}", json_str);
            eprintln!(
                "ImageGenResult keys:                  {:?}",
                parsed.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );
            eprintln!("webdav_path pattern (verify DFD):     {}", webdav_path);
            eprintln!("image_key format (verify DFD):        {}", parsed["image_key"]);
            eprintln!("Share URL presence (verify DFD §2j):  {}", parsed.get("share_url").is_some());
            eprintln!("Image bytes size (cached):            {} bytes", cached.image_bytes.len());
            eprintln!("MIME type (cached):                   {}", cached.mime_type.as_str());
            eprintln!("Total elapsed:                        {:.2?}", elapsed);
        }
        Err(e) => {
            panic!(
                "image_gen real test failed: {e}\n\
                 This could be a provider API issue (rate limit, auth, etc.) or a code bug.",
            );
        }
    }
}

/// Verify that data URIs in image_urls are correctly rejected or skipped by
/// the real provider (data URIs require uploading to provider CDN first, which
/// the tool handles internally via upload_data_uri).
///
/// This tests the data-URI-injection path from image-interception.md §2d,
/// where the harness injects data: URIs from user attachments into the
/// image_gen tool arguments.
#[ignore]
#[tokio::test]
async fn test_image_gen_real_data_uri_handling() {
    let provider_cfg = match load_image_provider("fal") {
        Some(cfg) => {
            eprintln!("Using image provider: fal");
            cfg
        }
        None => {
            eprintln!("Skipping: no image_provider config (fal) found in config.toml");
            return;
        }
    };

    let model = provider_cfg
        .models
        .get("gptimage_text")
        .cloned()
        .unwrap_or_else(|| "fal-ai/flux-pro/v1.1-ultra".into());

    let provider = match rockbot::FalAiProvider::new(&provider_cfg, &model) {
        Ok(p) => Box::new(p),
        Err(e) => {
            eprintln!("Skipping: failed to create provider: {e}");
            return;
        }
    };

    let webdav = match load_webdav_config() {
        Some(client) => {
            eprintln!("WebDAV connected");
            client
        }
        None => {
            eprintln!("Skipping: no WebDAV config found");
            return;
        }
    };

    let image_cache = std::sync::Arc::new(ImageCache::new());

    let tool = ImageGenTool::new(
        provider,
        "standard".into(),
        "png".into(),
        1,
        "square_1_1".into(),
        "2K".into(),
        webdav.clone(),
        image_cache.clone(),
    );

    // Read the test image from _docs/test_suite/p0.png and encode as data URI.
    // This exercises the real file-read + base64-encode path that the harness
    // uses when downloading RocketChat attachments (harness.rs:download_attachment_refs).
    let png_path = workspace_root().join("_docs/test_suite/p0.png");
    let png_bytes = match std::fs::read(&png_path) {
        Ok(bytes) => {
            eprintln!("Read test image: {} ({} bytes)", png_path.display(), bytes.len());
            bytes
        }
        Err(e) => {
            eprintln!("Skipping: cannot read test image at {}: {}", png_path.display(), e);
            return;
        }
    };
    let png_data_uri = format!(
        "data:image/png;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes)
    );
    eprintln!("Encoded test image as data URI ({} chars)", png_data_uri.len());

    let args = serde_json::json!({
        "prompt": "Make this red pixel image into a larger red square",
        "room_id": "test-room-image-gen-real",
        "webdav_dir": "test-image-gen-real",
        "image_cache_key": "test-call-id-data-uri",
        "image_urls": [&png_data_uri],
    });
    eprintln!("Executing image_gen with data URI in image_urls");

    let result = tool.execute(&args.to_string()).await;

    match result {
        Ok(json_str) => {
            eprintln!("Result JSON: {}", json_str);
            let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
            assert_eq!(parsed["ok"], true);

            // Clean up
            if let Some(path) = parsed["webdav_path"].as_str() {
                let _ = webdav.delete(path).await;
                eprintln!("Cleaned up WebDAV file: {}", path);
            }

            // Collect data shapes
            eprintln!("\n--- Data Shape Collection ---");
            eprintln!("Data URI handled successfully by image_gen. The tool uploaded the data URI to the provider CDN first, then used the resulting HTTPS URL for image editing.");
        }
        Err(e) => {
            eprintln!("image_gen with data URI failed: {e}");
            eprintln!("This may be expected if the provider does not support data URIs in image edit requests.");
            panic!("image_gen data URI test: {e}");
        }
    }
}
