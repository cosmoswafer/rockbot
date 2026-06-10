use std::env;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{Level, debug, error, info, warn};
use tracing_subscriber::FmtSubscriber;

use rockbot::config::AppConfig;
use rockbot::harness::AgentHarness;
use rockbot::provider::{AiProvider, DeepSeekProvider, OpenRouterProvider};
use rockbot::tool::ToolRegistry;
use rockbot::tools::{VisionTool, WebDavTool, WebFetchTool, WebSearchTool};

fn setup_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .without_time()
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

fn main() {
    setup_logging();

    let config_path = env::var("CONFIG_FILE")
        .ok()
        .unwrap_or_else(|| "config.toml".to_string());

    info!("Loading config from {}", config_path);

    let config = match AppConfig::from_file(&config_path) {
        Ok(c) => {
            info!("Config loaded successfully");
            c
        }
        Err(e) => {
            error!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        if let Err(e) = run_bot(config).await {
            error!("Bot exited with error: {}", e);
            std::process::exit(1);
        }
        info!("Bot shutdown complete");
    });
}

async fn run_bot(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let provider: Box<dyn AiProvider> = {
        let provider_name = &config.rocketchat.model.default_provider;
        let model_alias = &config.rocketchat.model.default_model;

        let provider_config = config
            .find_provider(provider_name)
            .ok_or_else(|| format!("Provider '{}' not found in config", provider_name))?;

        let resolved_model = config
            .resolve_model(provider_name, model_alias)
            .unwrap_or_else(|| model_alias.clone());

        info!(
            "Using provider '{}' with model '{}'",
            provider_name, resolved_model
        );

        match provider_name.as_str() {
            "deepseek" => {
                let p = DeepSeekProvider::new(provider_config, &resolved_model)?;
                Box::new(p)
            }
            "openrouter" => {
                let p = OpenRouterProvider::new(provider_config, &resolved_model)?;
                Box::new(p)
            }
            other => {
                return Err(format!("Unknown provider: {}", other).into());
            }
        }
    };

    let webdav = match &config.webdav {
        Some(cfg) => {
            info!("WebDAV config loaded");
            Some(cfg.create_client()?)
        }
        None => {
            warn!("No WebDAV config found, running without persistent storage");
            None
        }
    };

    let exa_key = config
        .tools
        .get("exa")
        .map(|t| t.api_key.clone())
        .or_else(|| env::var("EXA_API_KEY").ok())
        .unwrap_or_default();

    let mut harness = AgentHarness::new(config, provider, webdav.clone());

    let mut tool_registry = ToolRegistry::new();
    let has_exa = !exa_key.is_empty();
    tool_registry.register(Box::new(WebSearchTool::new(exa_key.clone())));
    if has_exa {
        tool_registry.register(Box::new(WebFetchTool::with_exa_key(exa_key)));
    } else {
        tool_registry.register(Box::new(WebFetchTool::new()));
    }
    tool_registry.register(Box::new(VisionTool::new()));
    if let Some(ref webdav_client) = webdav {
        tool_registry.register(Box::new(WebDavTool::new(webdav_client.clone())));
    }

    if !tool_registry.is_empty() {
        info!(
            "Registered {} tools: {:?}",
            tool_registry.len(),
            tool_registry.names()
        );
    }

    harness = harness.with_tools(tool_registry);
    let harness = Arc::new(Mutex::new(harness));

    info!("Bot initialized. Starting RocketChat connection...");

    let bot_name = {
        let h = harness.lock().await;
        format!("@{}", h.config().rocketchat.server.username)
    };

    let rocketchat_config = {
        let h = harness.lock().await;
        rocketchat::RocketChatConfig {
            server: rocketchat::config::ServerConfig {
                url: h.config().rocketchat.server.url.clone(),
                username: h.config().rocketchat.server.username.clone(),
                password: h.config().rocketchat.server.password.clone(),
                debug: h.config().rocketchat.server.debug,
                use_tls: true,
            },
        }
    };

    let client = rocketchat::RocketChatClient::new(rocketchat_config);

    client
        .connect_and_run(move |msg, sender| {
            let harness = harness.clone();
            let bot_name = bot_name.clone();
            async move {
                let mut h = harness.lock().await;

                let text = if msg.is_dm {
                    msg.text.clone()
                } else {
                    msg.text
                        .strip_prefix(&bot_name)
                        .unwrap_or(&msg.text)
                        .trim()
                        .to_string()
                };

                let room_name = if msg.room_name.is_empty() {
                    format!("dm-{}", msg.sender_name)
                } else {
                    msg.room_name.clone()
                };

                debug!(
                    "Processing message from {} in {} (is_dm={}): {}",
                    msg.sender_name, room_name, msg.is_dm, text
                );

                match h
                    .process_message(&msg.room_id, &room_name, msg.is_dm, &msg.sender_name, &text)
                    .await
                {
                    Ok(Some(reply)) => {
                        if let Err(e) = sender.reply(&reply).await {
                            error!("Failed to send reply: {}", e);
                        }
                        if let Err(e) = h.archive_room_if_needed(&msg.room_id).await {
                            warn!("Memory archiving failed: {}", e);
                        }
                    }
                    Ok(None) => {
                        if let Err(e) = h.archive_room_if_needed(&msg.room_id).await {
                            warn!("Memory archiving failed: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to process message: {}", e);
                        let _ = sender
                            .reply(&format!("Error processing message: {}", e))
                            .await;
                    }
                }
            }
        })
        .await?;

    Ok(())
}
