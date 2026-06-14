use std::env;
use std::path::PathBuf;

use rockbot::config::AppConfig;
use rockbot::provider::{AiProvider, DeepSeekProvider, OpenRouterProvider};
use rockbot::types::{ChatMessage, ChatRequest};

fn project_root() -> PathBuf {
    let mut dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        if dir.join("default.config.toml").exists() {
            return dir;
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            return PathBuf::from(".");
        }
    }
}

fn config_path() -> PathBuf {
    let root = project_root();
    env::var("CONFIG_FILE")
        .ok()
        .and_then(|p| {
            let path = PathBuf::from(&p);
            Some(if path.is_absolute() { path } else { root.join(p) })
        })
        .unwrap_or_else(|| root.join("config.toml"))
}

async fn create_provider(name: &str) -> Option<Box<dyn AiProvider>> {
    // Set cwd to project root so default.config.toml can be found
    let root = project_root();
    let _ = env::set_current_dir(&root);

    let path = config_path();
    let cfg = match AppConfig::from_file(path.to_str().unwrap_or("config.toml")) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config from {}: {e}", path.display());
            return None;
        }
    };

    match name {
        "deepseek" => {
            let provider_cfg = cfg.chat_providers.iter()
                .find(|p| p.name.as_str() == "deepseek")?;
            // Use a valid model name for the probe
            let model = provider_cfg.models.get("deepseek")
                .cloned()
                .unwrap_or_else(|| "deepseek-v4-pro".into());
            match DeepSeekProvider::new(&provider_cfg.clone(), &model) {
                Ok(p) => Some(Box::new(p)),
                Err(e) => { eprintln!("DeepSeek init failed: {e}"); None }
            }
        }
        "openrouter" => {
            let provider_cfg = cfg.chat_providers.iter()
                .find(|p| p.name.as_str() == "openrouter")?;
            let model = "qwen/qwen3.7-plus".to_string();
            match OpenRouterProvider::new(&provider_cfg.clone(), &model) {
                Ok(p) => Some(Box::new(p)),
                Err(e) => { eprintln!("OpenRouter init failed: {e}"); None }
            }
        }
        _ => None,
    }
}

/// Probe: send a short message and collect token usage
#[tokio::test]
#[ignore]
async fn probe_token_usage_short_message() {
    for provider_name in &["deepseek", "openrouter"] {
        let Some(provider) = create_provider(provider_name).await else {
            eprintln!("Skipping {provider_name}: not configured");
            continue;
        };

        let request = ChatRequest {
            model: provider.model_name().to_string(),
            messages: vec![
                ChatMessage::system("You are a helpful assistant. Reply concisely."),
                ChatMessage::user("Hello, what is 2+2?"),
            ],
            tools: None,
            stream: false,
            temperature: Some(0.3),
            max_tokens: Some(50),
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let json_str = serde_json::to_string_pretty(&request).unwrap();
        let input_chars = json_str.len();

        match provider.complete(request).await {
            Ok(result) => {
                let output_chars = result.text.as_deref().unwrap_or("").len();
                println!("\n=== {provider_name} short message ===");
                println!("  Input JSON chars:  {input_chars}");
                println!("  Output chars:      {output_chars}");
                if let Some(ref usage) = result.usage {
                    println!("  Actual token usage from provider:");
                    println!("    prompt_tokens:      {}", usage.prompt_tokens);
                    println!("    completion_tokens:  {}", usage.completion_tokens);
                    println!("    total_tokens:       {}", usage.total_tokens);
                    println!("    char/token ratio:   {:.2}", 
                        if usage.total_tokens > 0 { input_chars as f64 / usage.total_tokens as f64 } else { 0.0 }
                    );
                } else {
                    println!("(no usage info)");
                }
            }
            Err(e) => {
                eprintln!("  {provider_name} FAILED: {e}");
            }
        }
    }
}

/// Probe: send a medium-length conversation and collect token usage
#[tokio::test]
#[ignore]
async fn probe_token_usage_medium() {
    for provider_name in &["deepseek", "openrouter"] {
        let Some(provider) = create_provider(provider_name).await else {
            eprintln!("Skipping {provider_name}: not configured");
            continue;
        };

        let messages: Vec<ChatMessage> = vec![
            ChatMessage::system("You are a helpful assistant. Reply concisely."),
            ChatMessage::user("What is Rust?"),
            ChatMessage::assistant("Rust is a systems programming language focused on safety, speed, and concurrency."),
            ChatMessage::user("What are the main features?"),
            ChatMessage::assistant("Key features: zero-cost abstractions, move semantics, guaranteed memory safety, threads without data races."),
            ChatMessage::user("How does the borrow checker work?"),
        ];

        let request = ChatRequest {
            model: provider.model_name().to_string(),
            messages: messages.clone(),
            tools: None,
            stream: false,
            temperature: Some(0.3),
            max_tokens: Some(200),
            thinking: None,
            reasoning_effort: None,
            tool_choice: None,
        };

        let json_str = serde_json::to_string_pretty(&request).unwrap();
        let input_chars = json_str.len();

        match provider.complete(request).await {
            Ok(result) => {
                println!("\n=== {provider_name} medium conversation ===");
                println!("  Messages:          {}", messages.len());
                println!("  Input JSON chars:  {input_chars}");
                if let Some(ref usage) = result.usage {
                    println!("  Actual token usage:");
                    println!("    prompt_tokens:      {}", usage.prompt_tokens);
                    println!("    completion_tokens:  {}", usage.completion_tokens);
                    println!("    total_tokens:       {}", usage.total_tokens);
                    println!("    char/token ratio:   {:.2}",
                        if usage.total_tokens > 0 { input_chars as f64 / usage.total_tokens as f64 } else { 0.0 }
                    );
                } else {
                    println!("(no usage info)");
                }
            }
            Err(e) => {
                eprintln!("  {provider_name} FAILED: {e}");
            }
        }
    }
}
