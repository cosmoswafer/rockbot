use std::collections::HashMap;
use std::sync::{Arc, Once};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use crate::config::RocketChatConfig;
use crate::ddp;
use crate::error::{Result, RocketChatError};
use crate::types::{IncomingMessage, MessageFilter};

const SUBSCRIPTION_ID: &str = "ABCROCK";

const PING_INTERVAL_SECS: u64 = 30;
const READ_TIMEOUT_SECS: u64 = 300;
const TCP_KEEPALIVE_IDLE_SECS: u64 = 60;
const TCP_KEEPALIVE_INTERVAL_SECS: u64 = 10;

static INIT_CRYPTO: Once = Once::new();

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Clone)]
pub struct MessageSender {
    writer: Arc<Mutex<WriteHalf>>,
    room_id: String,
    user_id: String,
    auth_token: String,
}

impl MessageSender {
    fn new(writer: Arc<Mutex<WriteHalf>>, room_id: String, user_id: String, auth_token: String) -> Self {
        Self { writer, room_id, user_id, auth_token }
    }

    pub async fn reply(&self, text: &str) -> Result<()> {
        let payload = ddp::send_message_payload(&self.room_id, text);
        let mut writer = self.writer.lock().await;
        writer.send(&payload).await
    }

    pub async fn reply_with_alias(&self, text: &str, alias: &str) -> Result<()> {
        let payload = ddp::send_message_payload_with_alias(&self.room_id, text, Some(alias));
        let mut writer = self.writer.lock().await;
        writer.send(&payload).await
    }

    pub async fn reply_with_attachments(
        &self,
        text: &str,
        attachments: &[serde_json::Value],
        alias: Option<&str>,
    ) -> Result<()> {
        let payload =
            ddp::send_message_payload_with_attachments(&self.room_id, text, attachments, alias);
        let mut writer = self.writer.lock().await;
        writer.send(&payload).await
    }

    pub async fn reply_code(&self, text: &str) -> Result<()> {
        let formatted = format!("```\n{}\n```", text);
        self.reply(&formatted).await
    }

    pub async fn typing(&self, is_typing: bool, username: &str) -> Result<()> {
        let payload = ddp::typing_payload(&self.room_id, username, is_typing);
        let mut writer = self.writer.lock().await;
        writer.send(&payload).await
    }

    pub async fn set_real_name(&self, name: &str) -> Result<()> {
        let payload = ddp::set_real_name_payload(name);
        let mut writer = self.writer.lock().await;
        writer.send(&payload).await
    }

    pub async fn set_avatar(&self, avatar_url: &str) -> Result<()> {
        let payload = ddp::set_avatar_from_service_payload(avatar_url);
        let mut writer = self.writer.lock().await;
        writer.send(&payload).await
    }

    pub fn room_id(&self) -> &str {
        &self.room_id
    }

    pub fn user_id_for_rest(&self) -> &str {
        &self.user_id
    }

    pub fn auth_token_for_rest(&self) -> &str {
        &self.auth_token
    }

    pub fn rest_client(&self, config: &crate::config::RocketChatConfig) -> crate::rest::RestApiClient {
        crate::rest::RestApiClient::new(config, self.user_id.clone(), self.auth_token.clone())
    }
}

struct WriteHalf {
    inner: futures_util::stream::SplitSink<WsStream, Message>,
}

impl WriteHalf {
    async fn send(&mut self, payload: &serde_json::Value) -> Result<()> {
        let text = serde_json::to_string(payload)?;
        debug!("WS>>> {}", text);
        self.inner.send(Message::Text(text.into())).await?;
        Ok(())
    }

    async fn send_ping(&mut self) -> Result<()> {
        debug!("WS>>> PING");
        self.inner.send(Message::Ping(vec![].into())).await?;
        Ok(())
    }
}

fn set_tcp_keepalive(stream: &WsStream) {
    use std::os::unix::io::AsRawFd;

    let inner_maybe_tls = stream.get_ref();
    let tcp_ref: &tokio::net::TcpStream = inner_maybe_tls.get_ref();

    let ka = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(TCP_KEEPALIVE_IDLE_SECS))
        .with_interval(Duration::from_secs(TCP_KEEPALIVE_INTERVAL_SECS));

    let sock_ref = socket2::SockRef::from(tcp_ref);
    if let Err(e) = sock_ref.set_tcp_keepalive(&ka) {
        warn!("Failed to set TCP keepalive: {}", e);
    } else {
        let _raw_fd = tcp_ref.as_raw_fd();
        debug!("TCP keepalive set: idle={}s interval={}s fd={}", TCP_KEEPALIVE_IDLE_SECS, TCP_KEEPALIVE_INTERVAL_SECS, _raw_fd);
    }
}

pub struct RocketChatClient {
    config: RocketChatConfig,
    user_id: Option<String>,
    auth_token: Option<String>,
    username: String,
    bot_name: String,
    registered_rooms: HashMap<String, bool>,
}

impl RocketChatClient {
    pub fn new(config: RocketChatConfig) -> Self {
        let bot_name = format!("@{}", config.server.username);
        let username = config.server.username.clone();
        Self {
            config,
            user_id: None,
            auth_token: None,
            username,
            bot_name,
            registered_rooms: HashMap::new(),
        }
    }

    pub fn from_config_file(path: &str) -> Result<Self> {
        let config = RocketChatConfig::from_file(path)?;
        Ok(Self::new(config))
    }

    pub fn register_room(&mut self, room_name: &str) {
        self.registered_rooms.insert(room_name.to_string(), true);
    }

    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    pub fn bot_name(&self) -> &str {
        &self.bot_name
    }

    pub async fn connect_and_run<F, Fut>(mut self, handler: F) -> Result<()>
    where
        F: Fn(IncomingMessage, MessageSender) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        INIT_CRYPTO.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });

        let handler = Arc::new(handler);
        let uri = self.config.ws_uri()?;
        info!("Connecting to {}", uri);

        let (ws_stream, _response) = connect_async(&uri).await?;
        info!("WebSocket connected");
        set_tcp_keepalive(&ws_stream);

        let (write, mut read) = ws_stream.split();
        let writer = Arc::new(Mutex::new(WriteHalf { inner: write }));

        let connect_msg = ddp::connect_message();
        writer.lock().await.send(&connect_msg).await?;

        let _connected = Self::expect_msg(&mut read, "connected").await?;
        debug!("Connected response received");

        let login_msg =
            ddp::login_message(&self.config.server.username, &self.config.server.password);
        writer.lock().await.send(&login_msg).await?;

        let result = Self::expect_msg(&mut read, "result").await?;
        let (user_id, token) = ddp::extract_login_result(&result)
            .ok_or_else(|| RocketChatError::AuthFailed("Missing id/token in result".into()))?;
        info!("Login successful, user_id={}", user_id);
        self.user_id = Some(user_id.clone());
        self.auth_token = Some(token);

        let sub_msg = ddp::subscribe_message(SUBSCRIPTION_ID);
        writer.lock().await.send(&sub_msg).await?;

        let _ready = Self::expect_msg(&mut read, "ready").await?;
        info!("Subscription confirmed");

        let ping_writer = writer.clone();
        let ping_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(PING_INTERVAL_SECS)).await;
                let mut w = ping_writer.lock().await;
                if w.send_ping().await.is_err() {
                    debug!("WebSocket ping failed, ping task exiting");
                    break;
                }
            }
        });

        let bot_name = self.bot_name.clone();
        let _username = self.username.clone();
        let registered_rooms = self.registered_rooms.clone();

        info!("Entering event loop");
        loop {
            let frame_result = tokio::time::timeout(
                Duration::from_secs(READ_TIMEOUT_SECS),
                read.next(),
            )
            .await;

            let frame = match frame_result {
                Ok(Some(Ok(Message::Text(text)))) => text,
                Ok(Some(Ok(Message::Close(_)))) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => {
                    error!("WebSocket error: {}", e);
                    ping_task.abort();
                    return Err(e.into());
                }
                Ok(None) => {
                    info!("WebSocket stream ended");
                    break;
                }
                Err(_elapsed) => {
                    warn!(
                        "WebSocket read timeout (no frame for {}s) — treating as dead connection",
                        READ_TIMEOUT_SECS
                    );
                    ping_task.abort();
                    return Err(RocketChatError::ReadTimeout(READ_TIMEOUT_SECS));
                }
            };

            debug!("WS<<< {}", &frame);

            let value: serde_json::Value = match serde_json::from_str(&frame) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Failed to parse JSON: {}", e);
                    continue;
                }
            };

            if ddp::is_ping(&value) {
                let pong = ddp::pong_message();
                writer.lock().await.send(&pong).await?;
                debug!("Sent pong to server");
            } else if ddp::is_changed(&value) {
                let filter = MessageFilter::new(user_id.as_str());
                if let Some(msg) = filter.filter(&value) {
                    let should_dispatch = msg.is_dm
                        || (!msg.room_name.is_empty()
                            && (msg.text.starts_with(&bot_name)
                                || msg.text.contains(&bot_name)))
                        || (!registered_rooms.is_empty()
                            && !msg.room_name.is_empty()
                            && registered_rooms.contains_key(&msg.room_name));

                    if should_dispatch {
                        debug!(
                            "Dispatching message from {} in {} (dm={}, text='{}')",
                            msg.sender_name, msg.room_name, msg.is_dm, msg.text
                        );
                        let sender = MessageSender::new(writer.clone(), msg.room_id.clone(), user_id.to_string(), self.auth_token.as_deref().unwrap_or("").to_string());
                        let handler = handler.clone();
                        tokio::spawn(async move {
                            handler(msg, sender).await;
                        });
                    }
                }
            } else if ddp::is_nosub(&value)
                && ddp::subs_list(&value).iter().any(|s| s == SUBSCRIPTION_ID) {
                    warn!("Received nosub for stream-room-messages, re-subscribing");
                    let sub_msg = ddp::subscribe_message(SUBSCRIPTION_ID);
                    writer.lock().await.send(&sub_msg).await?;
                }
        }

        ping_task.abort();
        Ok(())
    }

    async fn expect_msg(
        read: &mut futures_util::stream::SplitStream<WsStream>,
        expected_msg: &str,
    ) -> Result<serde_json::Value> {
        loop {
            let frame = read
                .next()
                .await
                .ok_or(RocketChatError::Protocol("Connection closed".into()))?
                .map_err(|e| RocketChatError::WebSocket(Box::new(e)))?;

            match frame {
                Message::Text(text) => {
                    debug!("WS<<< {}", &text);
                    let value: serde_json::Value = serde_json::from_str(&text)?;
                    if ddp::msg_field(&value) == Some(expected_msg) {
                        return Ok(value);
                    }
                    if ddp::is_ping(&value) {
                        debug!("Received ping during setup, ignoring");
                        continue;
                    }
                }
                Message::Close(_) => {
                    return Err(RocketChatError::Protocol(
                        "Connection closed during setup".into(),
                    ));
                }
                _ => continue,
            }
        }
    }
}
