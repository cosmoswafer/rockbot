use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use base64::Engine;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::ruma::events::room::message::{
    MessageType, RoomMessageEventContent,
};
use matrix_sdk::ruma::events::SyncMessageLikeEvent;
use matrix_sdk::Client;
use tracing::{debug, info, warn};

use crate::error::{RockBotError, Result};
use super::{MessageHandler, MessagingClient, PlatformSender};

pub struct MatrixPlatform {
    homeserver: String,
    user_id: String,
    password: String,
    device_id: Option<String>,
    state_dir: String,
    force_relogin: AtomicBool,
}

impl MatrixPlatform {
    pub fn new(config: &crate::config::MatrixServerConfig) -> Self {
        Self {
            homeserver: config.homeserver.clone(),
            user_id: config.user_id.clone(),
            password: config.password.clone(),
            device_id: config.device_id.clone(),
            state_dir: config.state_dir.clone(),
            force_relogin: AtomicBool::new(false),
        }
    }
}

pub struct MatrixSender {
    room: matrix_sdk::Room,
    room_id: String,
    user_id: String,
}

impl MatrixSender {
    fn new(room: matrix_sdk::Room, user_id: String) -> Self {
        let room_id = room.room_id().to_string();
        Self { room, room_id, user_id }
    }
}

#[async_trait]
impl PlatformSender for MatrixSender {
    async fn send_reply(&self, text: &str, _alias: Option<&str>) -> Result<()> {
        let content = RoomMessageEventContent::text_markdown(text.to_string());
        self.room
            .send(content)
            .await
            .map_err(|e| RockBotError::Provider(format!("Matrix send failed: {e}")))?;
        Ok(())
    }

    async fn send_reply_with_attachments(
        &self,
        text: &str,
        _attachments: &[serde_json::Value],
        _alias: Option<&str>,
    ) -> Result<()> {
        warn!("Matrix: attachments not yet supported, sending text only");
        let content = RoomMessageEventContent::text_markdown(text.to_string());
        self.room
            .send(content)
            .await
            .map_err(|e| RockBotError::Provider(format!("Matrix send failed: {e}")))?;
        Ok(())
    }

    async fn send_typing(&self, typing: bool) -> Result<()> {
        self.room
            .typing_notice(typing)
            .await
            .map_err(|e| RockBotError::Provider(format!("Matrix typing failed: {e}")))?;
        Ok(())
    }

    fn room_id(&self) -> &str {
        &self.room_id
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn PlatformSender> {
        Box::new(MatrixSender::new(self.room.clone(), self.user_id.clone()))
    }

    fn strip_mention_prefix(&self, text: &str) -> String {
        strip_matrix_mention_prefix(text, &self.user_id)
    }
}

pub(crate) fn strip_matrix_mention_prefix(text: &str, bot_user_id: &str) -> String {
    let localpart_mention = bot_user_id
        .strip_prefix('@')
        .and_then(|s| s.split(':').next())
        .map(|local| format!("@{}", local))
        .unwrap_or_else(|| bot_user_id.to_string());

    text.strip_prefix(&format!("{} ", bot_user_id))
        .or_else(|| text.strip_prefix(bot_user_id))
        .or_else(|| text.strip_prefix(&format!("{} ", localpart_mention)))
        .or_else(|| text.strip_prefix(&localpart_mention))
        .unwrap_or(text)
        .trim()
        .to_string()
}

#[async_trait]
impl MessagingClient for MatrixPlatform {
    async fn connect_and_run(&self, handler: MessageHandler) -> Result<()> {
        let handler = std::sync::Arc::new(handler);

        std::fs::create_dir_all(&self.state_dir).ok();

        let client = Client::builder()
            .homeserver_url(&self.homeserver)
            .sqlite_store(&self.state_dir, None)
            .build()
            .await
            .map_err(|e| RockBotError::Config(format!("Matrix client build failed: {e}")))?;

        let need_login = self.force_relogin.swap(false, Ordering::SeqCst)
            || !client.matrix_auth().logged_in();

        let user_id_owned = if need_login {
            let login_builder =
                client.matrix_auth().login_username(&self.user_id, &self.password);
            let login_builder = if let Some(ref device_id) = self.device_id {
                login_builder.device_id(device_id)
            } else {
                login_builder
            };
            login_builder
                .send()
                .await
                .map_err(|e| RockBotError::AuthFailed(format!("Matrix login failed: {e}")))?;

            let user_id_owned = client
                .user_id()
                .map(|u| u.to_string())
                .ok_or_else(|| {
                    RockBotError::AuthFailed(
                        "Matrix: client.user_id() returned None after successful login".into(),
                    )
                })?;

            info!(
                "Matrix: logged in as {} (user_id={})",
                self.user_id, user_id_owned
            );
            user_id_owned
        } else {
            let user_id_owned = client
                .user_id()
                .map(|u| u.to_string())
                .ok_or_else(|| {
                    RockBotError::AuthFailed(
                        "Matrix: client.user_id() returned None for restored session".into(),
                    )
                })?;

            info!(
                "Matrix: session restored from state store (user_id={})",
                user_id_owned
            );
            user_id_owned
        };

        const HISTORICAL_GRACE_SECS: u64 = 600;

        let startup_ts_secs: u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let handler = handler.clone();
        let media_client = client.clone();

        client.add_event_handler(
            move |ev: SyncMessageLikeEvent<RoomMessageEventContent>,
                  room: matrix_sdk::Room| {
                let handler = handler.clone();
                let user_id = user_id_owned.clone();
                let media_client = media_client.clone();
                async move {
                    debug!("Matrix: received message event in room {}", room.room_id());

                    if room.state() != matrix_sdk::RoomState::Joined {
                        debug!("Matrix: ignoring message in non-joined room");
                        return;
                    }

                    let SyncMessageLikeEvent::Original(ref original) = ev else {
                        debug!("Matrix: ignoring non-original message event");
                        return;
                    };

                    let sender = original.sender.to_string();
                    if sender == user_id {
                        info!("Matrix: ignoring own message (user_id={})", user_id);
                        return;
                    }

                    let msg_ts_secs: u64 = original.origin_server_ts.as_secs().into();
                    if msg_ts_secs + HISTORICAL_GRACE_SECS < startup_ts_secs {
                        info!(
                            "Matrix: ignoring historical message (msg_ts={} + grace={} < startup_ts={})",
                            msg_ts_secs, HISTORICAL_GRACE_SECS, startup_ts_secs
                        );
                        return;
                    }

                    info!("Matrix: processing message from sender='{}' in room {}", sender, room.room_id());

                    let (body, attachments) = match &original.content.msgtype {
                        MessageType::Text(text) => {
                            (text.body.clone(), vec![])
                        }
                        MessageType::Image(image_content) => {
                            let mime_for_log = image_content
                                .info
                                .as_ref()
                                .and_then(|i| i.mimetype.as_deref())
                                .unwrap_or("image/png");
                            let source_for_log = format!("{:?}", image_content.source);
                            debug!(
                                "Matrix: m.image event body={:?} mime={} source={}",
                                image_content.body, mime_for_log, source_for_log
                            );
                            match media_client.media().get_file(image_content, false).await {
                                Ok(Some(bytes)) => {
                                    info!(
                                        "Matrix: downloaded image bytes={} mime={} body={:?}",
                                        bytes.len(),
                                        mime_for_log,
                                        image_content.body
                                    );
                                    let mime = mime_for_log;
                                    let data_uri = format!(
                                        "data:{};base64,{}",
                                        mime,
                                        base64::engine::general_purpose::STANDARD.encode(&bytes)
                                    );
                                    let title = image_content.body.clone();
                                    let att = rocketchat::AttachmentInfo {
                                        title: Some(title.clone()),
                                        title_link: Some(data_uri),
                                        title_link_download: None,
                                        image_url: None,
                                        image_type: Some(mime.to_string()),
                                        image_size: image_content.info.as_ref().and_then(|i| i.size.map(Into::into)),
                                        image_dimensions: image_content.info.as_ref().and_then(|i| {
                                            i.width.zip(i.height).map(|(w, h)| rocketchat::ImageDim {
                                                width: w.into(),
                                                height: h.into(),
                                            })
                                        }),
                                        image_preview: None,
                                        attach_type: Some("file".to_string()),
                                        file_id: None,
                                    };
                                    (title, vec![att])
                                }
                                Ok(None) => {
                                    warn!(
                                        "Matrix: m.image get_file returned Ok(None) — no bytes downloaded (body={:?}, mime={}, source={}). Possible cause: allow_redirect=false rejected a redirect, or media missing source/info.",
                                        image_content.body, mime_for_log, source_for_log
                                    );
                                    (image_content.body.clone(), vec![])
                                }
                                Err(e) => {
                                    warn!(
                                        "Matrix: m.image get_file failed (body={:?}, mime={}, source={}): {e:?}",
                                        image_content.body, mime_for_log, source_for_log
                                    );
                                    (image_content.body.clone(), vec![])
                                }
                            }
                        }
                        other => {
                            let msgtype_name = match other {
                                MessageType::File(_) => "m.file",
                                MessageType::Video(_) => "m.video",
                                MessageType::Audio(_) => "m.audio",
                                MessageType::Emote(_) => "m.emote",
                                MessageType::Notice(_) => "m.notice",
                                MessageType::Location(_) => "m.location",
                                MessageType::ServerNotice(_) => "m.server_notice",
                                _ => "<unknown>",
                            };
                            debug!(
                                "Matrix: ignoring msgtype={} (only m.text and m.image are handled)",
                                msgtype_name
                            );
                            return;
                        }
                    };

                    let room_id = room.room_id().to_string();
                    let room_name = room
                        .canonical_alias()
                        .map(|a| a.alias().to_string())
                        .unwrap_or_else(|| room_id.clone());

                    let room_fname = room
                        .display_name()
                        .await
                        .ok()
                        .and_then(|dn| match dn {
                            matrix_sdk::RoomDisplayName::Named(name) => Some(name),
                            matrix_sdk::RoomDisplayName::Calculated(name) => Some(name),
                            _ => None,
                        })
                        .unwrap_or_else(|| room_name.clone());

                    let is_dm = room.active_members_count() <= 2;
                    if !is_dm {
                        let localpart = user_id
                            .strip_prefix('@')
                            .and_then(|s| s.split(':').next())
                            .unwrap_or(&user_id);
                        let mention_at = format!("@{}", localpart);
                        // Check: body contains @localpart, @user_id, or m.mentions includes bot
                        let mentioned = body.contains(&mention_at)
                            || body.contains(&user_id)
                            || original
                                .content
                                .mentions
                                .as_ref()
                                .is_some_and(|m| m.user_ids.iter().any(|u| u.as_str() == user_id));
                        if !mentioned {
                            info!(
                                "Matrix: ignoring message without @mention (sender={} user_id={} localpart={} body={:?} mentions={:?} member_count={})",
                                sender, user_id, localpart, body,
                                original.content.mentions.as_ref().map(|m| &m.user_ids),
                                room.active_members_count()
                            );
                            return;
                        }
                        info!(
                            "Matrix: mention match (user_id={} localpart={} body={:?} mentions={:?})",
                            user_id, localpart, body,
                            original.content.mentions.as_ref().map(|m| &m.user_ids)
                        );
                    }

                    let sender_name = sender
                        .strip_prefix('@')
                        .and_then(|s| s.split(':').next())
                        .unwrap_or(&sender)
                        .to_string();

                    let ts_secs: u64 = original.origin_server_ts.as_secs().into();

                    let msg = rocketchat::IncomingMessage {
                        msg_id: Some(original.event_id.to_string()),
                        room_id: room_id.clone(),
                        room_name: room_name.clone(),
                        room_fname,
                        sender_name,
                        text: body,
                        is_dm,
                        timestamp: Some(ts_secs as i64),
                        sender_id: sender,
                        alias: None,
                        file: None,
                        files: vec![],
                        attachments,
                        urls: vec![],
                    };

                    let platform_sender: Box<dyn PlatformSender> =
                        Box::new(MatrixSender::new(room, user_id.clone()));
                    handler(msg, platform_sender).await;
                }
            },
        );

        info!("Matrix: starting sync loop...");

        client
            .sync(SyncSettings::default())
            .await
            .map_err(|e| {
                let is_token_error = e
                    .client_api_error_kind()
                    .is_some_and(|kind| matches!(kind, ErrorKind::UnknownToken(_)));
                if is_token_error {
                    self.force_relogin.store(true, Ordering::SeqCst);
                    warn!("Matrix: sync failed with M_UNKNOWN_TOKEN, forcing re-login on next connect");
                    RockBotError::AuthFailed(format!("Matrix sync error: {e}"))
                } else {
                    RockBotError::Provider(format!("Matrix sync error: {e}"))
                }
            })?;

        Ok(())
    }

    fn bot_id(&self) -> &str {
        &self.user_id
    }
}

#[cfg(test)]
mod tests {
    use super::strip_matrix_mention_prefix;

    #[test]
    fn test_strip_full_mxid_with_space() {
        assert_eq!(
            strip_matrix_mention_prefix("@rockbot:matrix.org hello", "@rockbot:matrix.org"),
            "hello"
        );
    }

    #[test]
    fn test_strip_localpart_with_space() {
        assert_eq!(
            strip_matrix_mention_prefix("@rockbot hello", "@rockbot:matrix.org"),
            "hello"
        );
    }

    #[test]
    fn test_strip_localpart_without_space() {
        assert_eq!(
            strip_matrix_mention_prefix("@rockbothello", "@rockbot:matrix.org"),
            "hello"
        );
    }

    #[test]
    fn test_strip_full_mxid_without_space() {
        assert_eq!(
            strip_matrix_mention_prefix("@rockbot:matrix.orghello", "@rockbot:matrix.org"),
            "hello"
        );
    }

    #[test]
    fn test_strip_mention_only() {
        assert_eq!(
            strip_matrix_mention_prefix("@rockbot", "@rockbot:matrix.org"),
            ""
        );
    }

    #[test]
    fn test_strip_no_match() {
        assert_eq!(
            strip_matrix_mention_prefix("hello world", "@rockbot:matrix.org"),
            "hello world"
        );
    }
}
