use std::collections::HashMap;
use std::path::PathBuf;

use rockbot::config::ProviderConfig;
use rockbot::provider::fal::FalAiProvider;
use rockbot::types::{ImageGenParams, ImageSizeValue};
use rockbot::validated::{ConfigUrl, ProviderName};

/// Find the workspace root by walking up from CARGO_MANIFEST_DIR.
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().to_path_buf()
}

/// Load the `fal` image provider config from the workspace `config.toml`.
fn load_fal_config() -> ProviderConfig {
    let config_path = workspace_root().join("config.toml");
    let contents = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("Cannot read config.toml at {:?}: {}", config_path, e));
    let toml: toml::Value = toml::from_str(&contents).unwrap();

    let providers = toml["image_providers"]
        .as_array()
        .expect("config.toml: [image_providers] array not found");

    for provider in providers {
        if provider["name"].as_str() == Some("fal") {
            let base_url = provider
                .get("base_url")
                .and_then(|v| v.as_str())
                .unwrap_or("https://queue.fal.run");
            let mut models = HashMap::new();
            if let Some(models_table) = provider.get("models").and_then(|v| v.as_table()) {
                for (k, v) in models_table {
                    if let Some(val) = v.as_str() {
                        models.insert(k.clone(), val.to_string());
                    }
                }
            }
            return ProviderConfig {
                name: ProviderName::try_new("fal".to_string()).unwrap(),
                api_key: provider["api_key"].as_str().unwrap_or("").into(),
                base_url: ConfigUrl::try_new(base_url.to_string()).unwrap(),
                basecf_url: None,
                chat_path: None,
                draw_path: None,
                models,
            };
        }
    }
    panic!("fal image_provider not found in config.toml");
}

#[ignore]
#[tokio::test]
async fn test_fal_image_edit_with_p1() {
    // Load fal config and resolve the edit model
    let fal_cfg = load_fal_config();
    let edit_model = fal_cfg
        .models
        .get("gptimage_edit")
        .cloned()
        .unwrap_or_else(|| "openai/gpt-image-2/edit".to_string());

    eprintln!("FAL base_url: {}", fal_cfg.base_url.as_str());
    eprintln!("FAL edit model: {}", edit_model);
    eprintln!("FAL api_key: {}...", &fal_cfg.api_key[..8]);

    let provider = FalAiProvider::new(&fal_cfg, &edit_model)
        .expect("Failed to create FalAiProvider");

    // Read p1.png
    let png_path = workspace_root().join("_doc/ref_img/p1.png");
    let png_bytes = std::fs::read(&png_path)
        .unwrap_or_else(|e| panic!("Cannot read p1.png at {:?}: {}", png_path, e));
    eprintln!("Read p1.png: {} bytes", png_bytes.len());

    // Upload to fal storage
    let uploaded_url = provider
        .upload_file(&png_bytes, "image/png")
        .await
        .expect("Failed to upload p1.png to fal storage");
    eprintln!("Uploaded to fal CDN: {}", uploaded_url);

    // Build edit params
    let mut params = ImageGenParams::new("Transform the attached image: Change the girl's red sweater outfit to Rei Ayanami's iconic blue plugsuit cosplay from Neon Genesis Evangelion. Keep her pose, face, and hairstyle as natural as possible.");
    params.image_size = Some(ImageSizeValue::Preset("landscape_4_3".into()));
    params.image_urls = Some(vec![uploaded_url]);

    // This call exercises submit_request → poll_status → fetch_result.
    // With the current buggy status URL, poll_status will fail with an empty 405 response.
    let result = provider.generate_image_url(&params).await;

    match result {
        Ok(url) => {
            eprintln!("Image generated successfully: {}", url);
            assert!(
                url.starts_with("https://"),
                "Result should be a HTTPS URL"
            );
        }
        Err(e) => {
            panic!(
                "fal.ai image edit FAILED. This likely confirms the status URL bug.\nError: {:?}\n\n\
                 The status poll URL is probably constructed incorrectly.\n\
                 Expected status URL pattern: {{base_url}}/{{model_no_action}}/requests/{{id}}/status\n\
                 Current code builds: {{base_url}}/{{model_id}}/requests/{{id}}/status\n\
                 where model_id='{}' includes the action suffix '/edit'.",
                e,
                provider.model_id()
            );
        }
    }
}
