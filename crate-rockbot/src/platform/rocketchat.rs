use std::any::Any;

use async_trait::async_trait;
use tracing::warn;

use crate::error::Result;
use super::{MessageHandler, MessagingClient, PlatformSender};

pub struct RocketChatPlatform {
    pub config: rocketchat::RocketChatConfig,
    pub bot_name: String,
}

impl RocketChatPlatform {
    pub fn new(
        config: rocketchat::RocketChatConfig,
        bot_name: String,
    ) -> Self {
        Self {
            config,
            bot_name,
        }
    }
}

pub struct RcPlatformSender {
    sender: rocketchat::MessageSender,
    username: String,
    rc_config: rocketchat::RocketChatConfig,
}

impl RcPlatformSender {
    pub fn new(
        sender: rocketchat::MessageSender,
        username: String,
        rc_config: rocketchat::RocketChatConfig,
    ) -> Self {
        Self {
            sender,
            username,
            rc_config,
        }
    }

    pub fn sender(&self) -> &rocketchat::MessageSender {
        &self.sender
    }

    pub fn rc_config(&self) -> &rocketchat::RocketChatConfig {
        &self.rc_config
    }
}

#[async_trait]
impl PlatformSender for RcPlatformSender {
    async fn send_reply(&self, text: &str, _alias: Option<&str>) -> Result<()> {
        self.sender
            .reply(text)
            .await
            .map_err(|e| crate::error::RockBotError::Provider(format!("DDP reply failed: {e}")))?;
        Ok(())
    }

    async fn send_reply_with_attachments(
        &self,
        text: &str,
        attachments: &[serde_json::Value],
        alias: Option<&str>,
    ) -> Result<()> {
        self.sender
            .reply_with_attachments(text, attachments, alias)
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!(
                    "DDP reply_with_attachments failed: {e}"
                ))
            })?;
        Ok(())
    }

    async fn send_typing(&self, typing: bool) -> Result<()> {
        if let Err(e) = self.sender.typing(typing, &self.username).await {
            warn!("Failed to send typing indicator: {}", e);
        }
        Ok(())
    }

    fn room_id(&self) -> &str {
        self.sender.room_id()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn PlatformSender> {
        Box::new(RcPlatformSender::new(
            self.sender.clone(),
            self.username.clone(),
            self.rc_config.clone(),
        ))
    }
}

#[async_trait]
impl MessagingClient for RocketChatPlatform {
    async fn connect_and_run(&self, handler: MessageHandler) -> Result<()> {
        let client = rocketchat::RocketChatClient::new(self.config.clone());

        let username = self.bot_name.trim_start_matches('@').to_string();
        let rc_config = self.config.clone();

        client
            .connect_and_run(move |msg, sender| {
                let ps: Box<dyn PlatformSender> = Box::new(RcPlatformSender::new(
                    sender,
                    username.clone(),
                    rc_config.clone(),
                ));
                handler(msg, ps)
            })
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!("RocketChat connection error: {e}"))
            })
    }
}
