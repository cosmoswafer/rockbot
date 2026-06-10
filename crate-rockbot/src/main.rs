use std::env;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::{Level, debug, error, info, warn};
use tracing_subscriber::FmtSubscriber;

use rockbot::config::AppConfig;
use rockbot::harness::AgentHarness;
use rockbot::provider::{AiProvider, DeepSeekProvider, FalAiProvider, OpenRouterProvider};
use rockbot::tool::ToolRegistry;
use rockbot::tools::{
    CalendarTool, DateTimeTool, EditSoulTool, ForgetKnowledgeTool, ImageGenTool,
    RecallKnowledgeTool, SaveKnowledgeTool, VisionTool, WebDavTool, WebFetchTool, WebSearchTool,
};

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
            .find_chat_provider(provider_name)
            .ok_or_else(|| format!("Provider '{}' not found in config", provider_name))?;

        let resolved_model = config
            .resolve_chat_model(provider_name, model_alias)
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

    let webdav_config_for_calendar = config.webdav.clone();

    let exa_key = config
        .tools
        .get("exa")
        .map(|t| t.api_key.clone())
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
    tool_registry.register(Box::new(DateTimeTool::new()));
    tool_registry.register(Box::new(VisionTool::new()));
    if let Some(ref webdav_client) = webdav {
        tool_registry.register(Box::new(WebDavTool::new(webdav_client.clone())));
        tool_registry.register(Box::new(EditSoulTool::new(webdav_client.clone())));
        tool_registry.register(Box::new(SaveKnowledgeTool::new(webdav_client.clone())));
        tool_registry.register(Box::new(ForgetKnowledgeTool::new(webdav_client.clone())));
        tool_registry.register(Box::new(RecallKnowledgeTool::new(webdav_client.clone())));
        info!("Registered knowledge tools (save, forget, recall)");

        if let Some(ref wd_cfg) = webdav_config_for_calendar {
            if let Some(calendar_tool) = CalendarTool::from_config(webdav_client.clone(), wd_cfg) {
                tool_registry.register(Box::new(calendar_tool));
                info!(
                    "Registered calendar tool for '{}'",
                    wd_cfg.calendar_name.as_deref().unwrap_or("?")
                );
            }
        }

        let (image_provider_name, image_model_name) = harness
            .config()
            .image_model
            .as_ref()
            .map(|im| (im.default_provider.as_str(), im.default_model.as_str()))
            .unwrap_or(("fal", "fal-ai/flux/schnell"));

        let image_cfg = harness.config().find_image_provider(image_provider_name);
        if let Some(img_cfg) = image_cfg {
            let resolved_model = harness
                .config()
                .resolve_image_model(image_provider_name, image_model_name)
                .unwrap_or_else(|| image_model_name.to_string());

            match FalAiProvider::new(img_cfg, &resolved_model) {
                Ok(fal_provider) => {
                    tool_registry.register(Box::new(ImageGenTool::new(
                        fal_provider,
                        webdav_client.clone(),
                    )));
                    info!(
                        "Registered image_gen tool with {} / {}",
                        image_provider_name, resolved_model
                    );
                }
                Err(e) => {
                    warn!("Failed to create image gen provider: {}", e);
                }
            }
        }
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

    // Spawn combined maintenance timer: persist dirty snapshots, then evict stale rooms
    {
        let harness = harness.clone();
        let persist_secs = {
            let h = harness.lock().await;
            h.memory().persist_interval_secs
        };
        let ttl_secs = {
            let h = harness.lock().await;
            h.config().rocketchat.model.memory_ttl_secs
        };
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(persist_secs.max(1)));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let mut h = harness.lock().await;
                h.maintenance_tick(ttl_secs).await;
            }
        });
        info!(
            "Maintenance timer started (persist every {}s, evict after {}s idle)",
            persist_secs, ttl_secs
        );
    }

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

    let max_retries: u32 = 5;
    let mut retry_count: u32 = 0;

    let shutdown = tokio::signal::ctrl_c();

    tokio::pin!(shutdown);

    loop {
        let client = rocketchat::RocketChatClient::new(rocketchat_config.clone());

        let connect_fut = client
            .connect_and_run({
                let harness = harness.clone();
                let bot_name = bot_name.clone();
                move |msg, sender| {
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
                            msg.sender_name.clone()
                        } else {
                            msg.room_name.clone()
                        };

                        let display_name = if msg.room_fname.is_empty() {
                            room_name.clone()
                        } else {
                            msg.room_fname.clone()
                        };

                        debug!(
                            "Processing message from {} in {} (is_dm={}): {}",
                            msg.sender_name, display_name, msg.is_dm, text
                        );

                        match h
                            .process_message(
                                &msg.room_id,
                                &room_name,
                                &msg.room_fname,
                                msg.is_dm,
                                &msg.sender_name,
                                &text,
                            )
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
                }
            });

        let result = tokio::select! {
            r = connect_fut => r,
            _ = &mut shutdown => {
                info!("Received shutdown signal, exiting...");
                return Ok(());
            }
        };

        match result {
            Ok(()) => {
                info!("WebSocket connection closed normally");
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!(
                        "Max reconnect retries ({}) reached, shutting down",
                        max_retries
                    );
                    return Err(e.into());
                }
                let delay = Duration::from_secs(2u64.pow(retry_count));
                warn!(
                    "WebSocket disconnected: {}. Reconnecting in {:?} (attempt {}/{})",
                    e, delay, retry_count, max_retries
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    Ok(())
}
