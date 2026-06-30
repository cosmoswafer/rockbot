use std::any::Any;

use async_trait::async_trait;
use base64::Engine;
use matrix_sdk::config::SyncSettings;
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
}

impl MatrixPlatform {
    pub fn new(config: &crate::config::MatrixServerConfig) -> Self {
        Self {
            homeserver: config.homeserver.clone(),
            user_id: config.user_id.clone(),
            password: config.password.clone(),
            device_id: config.device_id.clone(),
        }
    }
}

pub struct MatrixSender {
    room: matrix_sdk::Room,
    room_id: String,
}

impl MatrixSender {
    fn new(room: matrix_sdk::Room) -> Self {
        let room_id = room.room_id().to_string();
        Self { room, room_id }
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
        Box::new(MatrixSender::new(self.room.clone()))
    }
}

#[async_trait]
impl MessagingClient for MatrixPlatform {
    async fn connect_and_run(&self, handler: MessageHandler) -> Result<()> {
        let handler = std::sync::Arc::new(handler);
        let client = Client::builder()
            .homeserver_url(&self.homeserver)
            .build()
            .await
            .map_err(|e| RockBotError::Config(format!("Matrix client build failed: {e}")))?;

        let login_builder = client.matrix_auth().login_username(&self.user_id, &self.password);
        let login_builder = if let Some(ref device_id) = self.device_id {
            login_builder.device_id(device_id)
        } else {
            login_builder
        };
        login_builder
            .send()
            .await
            .map_err(|e| RockBotError::AuthFailed(format!("Matrix login failed: {e}")))?;

        info!("Matrix: logged in as {}", self.user_id);

        let user_id_owned = client
            .user_id()
            .map(|u| u.to_string())
            .unwrap_or_default();

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
                        debug!("Matrix: ignoring own message");
                        return;
                    }

                    let msg_ts_secs: u64 = original.origin_server_ts.as_secs().into();
                    if msg_ts_secs < startup_ts_secs {
                        debug!(
                            "Matrix: ignoring historical message (msg_ts={} < startup_ts={})",
                            msg_ts_secs, startup_ts_secs
                        );
                        return;
                    }

                    info!("Matrix: processing message from sender='{}' in room {}", sender, room.room_id());

                    let (body, attachments) = match &original.content.msgtype {
                        MessageType::Text(text) => {
                            (text.body.clone(), vec![])
                        }
                        MessageType::Image(image_content) => {
                            match media_client.media().get_file(image_content, false).await {
                                Ok(Some(bytes)) => {
                                    let mime = image_content.info.as_ref()
                                        .and_then(|i| i.mimetype.as_deref())
                                        .unwrap_or("image/png");
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
                                _ => {
                                    // Download failed — treat as text-only
                                    (image_content.body.clone(), vec![])
                                }
                            }
                        }
                        _ => {
                            debug!("Matrix: ignoring non-text, non-image message");
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

                    let member_count = room.active_members_count();
                    let is_dm = member_count <= 2;

                    if !is_dm {
                        let localpart = user_id
                            .strip_prefix('@')
                            .and_then(|s| s.split(':').next())
                            .unwrap_or(&user_id);
                        let mentioned = body.contains(&format!("@{}", localpart))
                            || body.contains(&user_id);
                        if !mentioned {
                            debug!(
                                "Matrix: ignoring message without @mention in non-DM room (sender={})",
                                sender
                            );
                            return;
                        }
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
                        Box::new(MatrixSender::new(room));
                    handler(msg, platform_sender).await;
                }
            },
        );

        info!("Matrix: starting sync loop...");

        client
            .sync(SyncSettings::default())
            .await
            .map_err(|e| RockBotError::Provider(format!("Matrix sync error: {e}")))?;

        Ok(())
    }
}
