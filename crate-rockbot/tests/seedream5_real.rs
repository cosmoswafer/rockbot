use std::collections::HashMap;
use std::path::PathBuf;

use rockbot::config::ProviderConfig;
use rockbot::provider::fal::FalAiProvider;
use rockbot::types::{ImageGenParams, ImageSizeValue};
use rockbot::validated::{ConfigUrl, ProviderName};

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().to_path_buf()
}

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
async fn test_seedream5_t2i_with_safety_checker_disabled() {
    let fal_cfg = load_fal_config();
    let t2i_model = fal_cfg
        .models
        .get("seedream5")
        .cloned()
        .unwrap_or_else(|| "fal-ai/bytedance/seedream/v5/pro/text-to-image".to_string());

    eprintln!("FAL base_url: {}", fal_cfg.base_url.as_str());
    eprintln!("FAL seedream5 model: {}", t2i_model);
    eprintln!("FAL api_key: {}...", &fal_cfg.api_key[..8.min(fal_cfg.api_key.len())]);

    let provider = FalAiProvider::new(&fal_cfg, &t2i_model)
        .expect("Failed to create FalAiProvider for seedream5");

    assert!(provider.model_id().contains("seedream/v5"), "model_id should be seedream5");

    let mut params = ImageGenParams::new(
        "A simple geometric test pattern: a blue square on a white background, minimal, clean",
    );
    params.image_size = Some(ImageSizeValue::Preset("auto_2K".into()));
    params.output_format = Some("png".into());
    params.num_images = Some(1);
    params.enable_safety_checker = Some(false);

    eprintln!("Executing generate_image_url with params: prompt={:?} image_size={:?} enable_safety_checker=false", params.prompt, params.image_size);

    let t_start = std::time::Instant::now();
    let result = provider.generate_image_url(&params).await;

    match result {
        Ok(url) => {
            let elapsed = t_start.elapsed();
            eprintln!("seedream5 generate_image_url completed in {elapsed:.2?}");
            eprintln!("Result URL: {}", url);
            assert!(url.starts_with("https://"), "Result should be a HTTPS URL");

            eprintln!("Downloading image from generated URL...");
            let image_bytes = reqwest::get(&url)
                .await
                .expect("Failed to download generated image")
                .bytes()
                .await
                .expect("Failed to read image bytes");
            eprintln!("Downloaded image: {} bytes", image_bytes.len());
            assert!(!image_bytes.is_empty(), "Generated image bytes should not be empty");
            assert_eq!(&image_bytes[..4], &[137, 80, 78, 71],
                "Result should be a PNG image (PNG magic bytes)");
            eprintln!("PNG header validated OK");
        }
        Err(e) => {
            panic!("seedream5 generate_image_url FAILED.\nError: {:?}\n\nCheck that:\n- Fal API key is valid\n- seedream5 model is accessible\n- enable_safety_checker: false is accepted\n- image_size: auto_2K is accepted", e);
        }
    }
}

#[ignore]
#[tokio::test]
async fn test_seedream5_submit_request_shape() {
    let fal_cfg = load_fal_config();
    let t2i_model = fal_cfg
        .models
        .get("seedream5")
        .cloned()
        .unwrap_or_else(|| "fal-ai/bytedance/seedream/v5/pro/text-to-image".to_string());

    let provider = FalAiProvider::new(&fal_cfg, &t2i_model)
        .expect("Failed to create FalAiProvider");

    let mut params = ImageGenParams::new("test: a red apple on a white background");
    params.image_size = Some(ImageSizeValue::Preset("auto_2K".into()));
    params.output_format = Some("png".into());
    params.num_images = Some(1);
    params.enable_safety_checker = Some(false);

    let url = provider.generate_image_url(&params).await
        .expect("seedream5 image generation failed");

    eprintln!("=== seedream5 Response Shape ===");
    eprintln!("Generated URL: {}", url);
    eprintln!("=================================");

    assert!(url.starts_with("https://"));
}

#[ignore]
#[tokio::test]
async fn test_seedream5_edit_shape() {
    let fal_cfg = load_fal_config();
    let edit_model = fal_cfg
        .models
        .get("seedream5_edit")
        .cloned()
        .unwrap_or_else(|| "fal-ai/bytedance/seedream/v5/pro/edit".to_string());

    let provider = FalAiProvider::new(&fal_cfg, &edit_model)
        .expect("Failed to create FalAiProvider for seedream5_edit");

    eprintln!("FAL seedream5_edit model: {}", edit_model);

    let mut params = ImageGenParams::new("test: simple edit prompt");
    params.image_size = Some(ImageSizeValue::Preset("square".into()));
    params.output_format = Some("png".into());
    params.num_images = Some(1);

    // Note: enable_safety_checker is NOT sent for seedream5_edit
    // since the model guard checks for "seedream/v5" and edit is seedream/v5/pro/edit.
    // This test verifies edit works without the safety checker param.

    let url = provider.generate_image_url(&params).await
        .expect("seedream5_edit image generation failed");

    eprintln!("seedream5_edit URL: {}", url);
    assert!(url.starts_with("https://"));
}
