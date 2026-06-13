use std::env;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

use rockbot::config::AppConfig;
use rockbot::harness::AgentHarness;
use rockbot::image_cache::ImageCache;
use rockbot::provider::{AiProvider, DeepSeekProvider, FalAiProvider, ImageProvider, OpenRouterImageProvider, OpenRouterProvider};
use rockbot::tool::ToolRegistry;
use rockbot::tools::{
    CalendarTool, DateTimeTool, EditSoulTool, ForgetKnowledgeTool, ImageGenTool,
    RecallKnowledgeTool, SaveKnowledgeTool, VisionTool, WebDavTool, WebFetchTool, WebSearchTool,
};
use rockbot::utils::strip_markdown_image_id;

fn setup_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
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
            .ok_or_else(|| format!("Provider '{}' not found in config", provider_name.as_str()))?;

        let resolved_model = config
            .resolve_chat_model(provider_name, model_alias)
            .unwrap_or_else(|| model_alias.clone());

        info!(
            "Using provider '{}' with model '{}'",
            provider_name.as_str(), resolved_model
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

    let image_cache = Arc::new(ImageCache::new());
    let mut harness = AgentHarness::new(config, provider, webdav.clone(), image_cache.clone());

    let mut tool_registry = ToolRegistry::new();
    let has_exa = !exa_key.is_empty();
    tool_registry.register(Box::new(WebSearchTool::new(exa_key.clone())));
    if has_exa {
        if let Some(ref webdav_client) = webdav {
            tool_registry.register(Box::new(WebFetchTool::with_exa_key_and_webdav(exa_key, webdav_client.clone())));
            debug!("WebFetchTool registered with Exa verification and WebDAV support");
        } else {
            tool_registry.register(Box::new(WebFetchTool::with_exa_key(exa_key)));
            debug!("WebFetchTool registered with Exa verification support");
        }
    } else {
        if let Some(ref webdav_client) = webdav {
            tool_registry.register(Box::new(WebFetchTool::with_webdav(webdav_client.clone())));
            debug!("WebFetchTool registered with WebDAV support (no Exa)");
        } else {
            tool_registry.register(Box::new(WebFetchTool::new()));
            debug!("WebFetchTool registered without Exa or WebDAV support");
        }
    }
    tool_registry.register(Box::new(DateTimeTool::new()));
    tool_registry.register(Box::new(VisionTool::with_max_bytes(
        harness.config().rocketchat.model.max_attachment_bytes,
    )));
    if let Some(ref webdav_client) = webdav {
        tool_registry.register(Box::new(WebDavTool::new(webdav_client.clone())));
        tool_registry.register(Box::new(EditSoulTool::new(webdav_client.clone())));
        tool_registry.register(Box::new(SaveKnowledgeTool::new(webdav_client.clone())));
        tool_registry.register(Box::new(ForgetKnowledgeTool::new(webdav_client.clone())));
        tool_registry.register(Box::new(RecallKnowledgeTool::new(webdav_client.clone())));
        info!("Registered WebDAV tools (webdav, edit_soul, knowledge)");

        if let Some(ref wd_cfg) = webdav_config_for_calendar {
            let calendar_tool = CalendarTool::from_config(webdav_client.clone(), wd_cfg);
            tool_registry.register(Box::new(calendar_tool));
            info!("Registered calendar tool (per-room, auto-created)");
        } else {
            debug!("Calendar tool not registered — WebDAV config missing calendar settings");
        }

        let (image_provider_name, t2i_model_name, edit_model_name, default_quality, default_output_format, default_num_images, _default_image_size, default_image_size_tier) = {
            let im = &harness.config().image_model;
            (
                im.default_provider.as_str(),
                im.default_text_model.as_str(),
                im.default_edit_model.as_str(),
                im.default_quality.as_str(),
                im.default_output_format.as_str(),
                im.default_num_images,
                im.default_image_size.as_str(),
                im.default_image_size_tier.as_str(),
            )
        };

        let image_cfg = harness.config().find_image_provider(image_provider_name);
        if let Some(img_cfg) = image_cfg {
            debug!("Found image provider '{}', resolving models t2i={} edit={}", image_provider_name, t2i_model_name, edit_model_name);
            let resolved_t2i = harness
                .config()
                .resolve_image_model(image_provider_name, t2i_model_name)
                .unwrap_or_else(|| t2i_model_name.to_string());

            let resolved_edit = harness
                .config()
                .resolve_image_model(image_provider_name, edit_model_name)
                .unwrap_or_else(|| edit_model_name.to_string());

            // Providers like OpenRouter use the same model for both t2i and edit.
            // Providers like Fal use different models for each.
            let same_provider = resolved_t2i == resolved_edit;

            debug!("Resolved image models: t2i='{}' edit='{}'", resolved_t2i, resolved_edit);

            let t2i_provider: Option<Box<dyn ImageProvider>> = match image_provider_name {
                "fal" => FalAiProvider::new(img_cfg, &resolved_t2i).ok().map(|p| Box::new(p) as Box<dyn ImageProvider>),
                "openrouter" => OpenRouterImageProvider::new(img_cfg, &resolved_t2i).ok().map(|p| Box::new(p) as Box<dyn ImageProvider>),
                other => {
                    warn!("Unknown image provider '{}' — image_gen tool not registered", other);
                    None
                }
            };

            if let Some(t2i) = t2i_provider {
                let edit_provider: Option<Box<dyn ImageProvider>> = {
                    let same_match = if same_provider {
                        match image_provider_name {
                            "fal" => FalAiProvider::new(img_cfg, &resolved_edit).ok().map(|p| Box::new(p) as Box<dyn ImageProvider>),
                            "openrouter" => OpenRouterImageProvider::new(img_cfg, &resolved_edit).ok().map(|p| Box::new(p) as Box<dyn ImageProvider>),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if same_provider && same_match.is_none() {
                        warn!("Failed to create same-provider edit provider ({}) — image_gen tool not registered", resolved_edit);
                    }
                    if same_provider {
                        same_match
                    } else {
                        match image_provider_name {
                            "fal" => FalAiProvider::new(img_cfg, &resolved_edit).ok().map(|p| Box::new(p) as Box<dyn ImageProvider>),
                            "openrouter" => OpenRouterImageProvider::new(img_cfg, &resolved_edit).ok().map(|p| Box::new(p) as Box<dyn ImageProvider>),
                            _ => {
                                warn!("image_gen tool not registered — unknown edit provider '{}'", image_provider_name);
                                None
                            }
                        }
                    }
                };

                if let Some(edit) = edit_provider {
                    info!(
                        "Registered image_gen with t2i={} / edit={}{}",
                        resolved_t2i,
                        resolved_edit,
                        if same_provider { " (same provider)" } else { "" }
                    );
                    tool_registry.register(Box::new(ImageGenTool::with_img2img(
                        t2i,
                        edit,
                        default_quality.to_string(),
                        default_output_format.to_string(),
                        default_num_images,
                        default_image_size_tier.to_string(),
                        webdav_client.clone(),
                        image_cache.clone(),
                    )));
                } else {
                    warn!("image_gen tool not registered — failed to create edit provider for '{}'", resolved_edit);
                }
            }
        } else {
            debug!("Image provider '{}' not found in config — image_gen tool not registered", image_provider_name);
        }
    } else {
        debug!("WebDAV not configured — WebDAV-dependent tools (webdav, edit_soul, knowledge, calendar, image_gen) not registered");
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
    let timer_handle = {
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
        })
    };
    info!(
        "Maintenance timer started (persist every {}s, evict after {}s idle)",
        {
            let h = harness.lock().await;
            h.memory().persist_interval_secs
        },
        {
            let h = harness.lock().await;
            h.config().rocketchat.model.memory_ttl_secs
        }
    );

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
                use_tls: true,
            },
        }
    };

    let max_retries: u32 = 5;
    let mut retry_count: u32 = 0;

    let shutdown = async {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
    };

    tokio::pin!(shutdown);

    loop {
        let mut client = rocketchat::RocketChatClient::new(rocketchat_config.clone());
        // Set display name for stage-4 filter (response to own display name in channels)
        {
            let h = harness.lock().await;
            let display_name = h.memory().any_display_name();
            client.set_display_name(display_name);
        }

        let connect_fut = client
            .connect_and_run({
                let harness = harness.clone();
                let bot_name = bot_name.clone();
                move |msg, sender| {
                    let harness = harness.clone();
                    let bot_name = bot_name.clone();
                    async move {
                        let username = bot_name.trim_start_matches('@').to_string();
                        if let Err(e) = sender.typing(true, &username).await {
                            warn!("Failed to send typing indicator: {}", e);
                        }

                        let hb_sender = sender.clone();
                        let hb_username = username.clone();
                        let heartbeat = tokio::spawn(async move {
                            loop {
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                if hb_sender.typing(true, &hb_username).await.is_err() {
                                    break;
                                }
                            }
                        });

                        let mut h = harness.lock().await;

                        // Initialize REST client on first message
                        if !h.has_rest_client() {
                            let rc_config = rocketchat::RocketChatConfig {
                                server: rocketchat::config::ServerConfig {
                                    url: h.config().rocketchat.server.url.clone(),
                                    username: h.config().rocketchat.server.username.clone(),
                                    password: h.config().rocketchat.server.password.clone(),
                                    use_tls: true,
                                },
                            };
                            h.set_rest_client(sender.rest_client(&rc_config));
                        }

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

                        // REST API fallback for Unicode room names
                        let display_name = if !msg.is_dm && (display_name.is_empty() || display_name == room_name) {
                            if let Some(fname) = h.resolve_room_fname(&msg.room_id).await {
                                fname
                            } else {
                                display_name
                            }
                        } else {
                            display_name
                        };

                        debug!(
                            "Processing message from {} in {} (is_dm={}): {}",
                            msg.sender_name, display_name, msg.is_dm, text
                        );

                        match h
                            .process_message(
                                &msg.room_id,
                                &room_name,
                                &display_name,
                                msg.is_dm,
                                &msg.sender_name,
                                &text,
                                &msg.attachments,
                                &msg.urls,
                            )
                            .await
                        {
                            Ok(Some(reply)) => {
                                heartbeat.abort();
                                if let Err(e) = sender.typing(false, &username).await {
                                    warn!("Failed to stop typing indicator: {}", e);
                                }
                                tokio::time::sleep(Duration::from_millis(300)).await;

                                // Check for generated image attachments
                                let image_ids = h.take_last_image_ids();
                                let mut attachments: Vec<serde_json::Value> = Vec::new();
                                let mut reply_text = reply.clone();

                                for image_id in &image_ids {
                                    if let Some(img) = h.take_image(image_id) {
                                        reply_text = strip_markdown_image_id(
                                            &reply_text,
                                            image_id,
                                        );
                                        if let Some(ref share_url) = img.share_url {
                                            // Prefer NextCloud share URL (short, works with REST)
                                            let markdown = format!(
                                                "\n\n![Generated image]({})",
                                                share_url
                                            );
                                            reply_text.push_str(&markdown);
                                        } else {
                                            // Fallback: data URI as DDP attachment
                                            let data_uri = img.data_uri();
                                            attachments.push(serde_json::json!({
                                                "image_url": data_uri
                                            }));
                                        }
                                    }
                                }

                                let has_images = !image_ids.is_empty();
                                let has_attachments = !attachments.is_empty();
                                let final_reply = if has_images {
                                    &reply_text
                                } else {
                                    &reply
                                };

                                if has_attachments {
                                    let alias = h
                                        .memory()
                                        .self_display_name(&msg.room_id);
                                    if let Err(e) = sender
                                        .reply_with_attachments(
                                            final_reply,
                                            &attachments,
                                            alias.as_deref(),
                                        )
                                        .await
                                    {
                                        error!(
                                            "Failed to send reply with attachments: {}",
                                            e
                                        );
                                    }
                                } else {
                                    // Try REST API with alias first, fall back to DDP
                                    let alias = h.memory().self_display_name(&msg.room_id);
                                    let rest_ok = if let Some(ref alias_name) = alias {
                                        debug!("Sending with alias={:?} via REST", alias_name);
                                        let rc_config = rocketchat::RocketChatConfig {
                                            server: rocketchat::config::ServerConfig {
                                                url: h.config().rocketchat.server.url.clone(),
                                                username: h.config().rocketchat.server.username.clone(),
                                                password: String::new(),
                                                use_tls: true,
                                            },
                                        };
                                        let rest = sender.rest_client(&rc_config);
                                        match rest.send_message(&msg.room_id, final_reply, Some(alias_name)).await {
                                            Ok(msg_id) => {
                                                debug!("REST send_message ok, msg_id={} alias={:?}", msg_id, alias_name);
                                                true
                                            }
                                            Err(e) => {
                                                warn!("REST send_message failed: {}, falling back to DDP", e);
                                                false
                                            }
                                        }
                                    } else {
                                        debug!("No self_display_name for room={}, sending via DDP without alias", msg.room_id);
                                        false
                                    };

                                    if !rest_ok {
                                        if let Err(e) = sender.reply(final_reply).await {
                                            error!("Failed to send reply: {}", e);
                                        }
                                    }
                                }
                                if let Err(e) = h.archive_room_if_needed(&msg.room_id).await {
                                    warn!("Memory archiving failed: {}", e);
                                }
                            }
                            Ok(None) => {
                                heartbeat.abort();
                                if let Err(e) = sender.typing(false, &username).await {
                                    warn!("Failed to stop typing indicator: {}", e);
                                }
                                if let Err(e) = h.archive_room_if_needed(&msg.room_id).await {
                                    warn!("Memory archiving failed: {}", e);
                                }
                            }
                            Err(e) => {
                                heartbeat.abort();
                                if let Err(te) = sender.typing(false, &username).await {
                                    warn!("Failed to stop typing indicator: {}", te);
                                }
                                error!("Failed to process message: {}", e);
                                if let Err(re) = sender
                                    .reply(&format!("Error processing message: {}", e))
                                    .await {
                                    warn!("Failed to send error reply: {}", re);
                                }
                                if let Err(e) = h.archive_room_if_needed(&msg.room_id).await {
                                    warn!("Memory archiving failed: {}", e);
                                }
                            }
                        }
                    }
                }
            });

        let result = tokio::select! {
            r = connect_fut => r,
            _ = &mut shutdown => {
                info!("Received shutdown signal, flushing snapshots...");
                timer_handle.abort();
                let mut h = harness.lock().await;
                h.flush_all_snapshots().await;
                info!("Graceful shutdown complete");
                return Ok(());
            }
        };

        match result {
            Ok(()) => {
                info!("WebSocket connection closed normally");
                timer_handle.abort();
                let mut h = harness.lock().await;
                h.flush_all_snapshots().await;
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!(
                        "Max reconnect retries ({}) reached, shutting down",
                        max_retries
                    );
                    timer_handle.abort();
                    let mut h = harness.lock().await;
                    h.flush_all_snapshots().await;
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
