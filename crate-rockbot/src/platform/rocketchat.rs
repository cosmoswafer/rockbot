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

    fn strip_mention_prefix(&self, text: &str) -> String {
        strip_rc_mention_prefix(text, &self.username)
    }
}

pub(crate) fn strip_rc_mention_prefix(text: &str, username: &str) -> String {
    let prefix = format!("@{}", username);
    text.strip_prefix(&format!("{} ", prefix))
        .or_else(|| text.strip_prefix(&prefix))
        .unwrap_or(text)
        .trim()
        .to_string()
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

    fn bot_id(&self) -> &str {
        &self.bot_name
    }
}

#[cfg(test)]
mod tests {
    use super::strip_rc_mention_prefix;

    #[test]
    fn test_strip_mention_with_space() {
        assert_eq!(strip_rc_mention_prefix("@rockai hello", "rockai"), "hello");
    }

    #[test]
    fn test_strip_mention_without_space() {
        assert_eq!(strip_rc_mention_prefix("@rockaihello", "rockai"), "hello");
    }

    #[test]
    fn test_strip_mention_only() {
        assert_eq!(strip_rc_mention_prefix("@rockai", "rockai"), "");
    }

    #[test]
    fn test_strip_mention_no_match() {
        assert_eq!(strip_rc_mention_prefix("hello world", "rockai"), "hello world");
    }

    #[test]
    fn test_strip_mention_leading_spaces() {
        assert_eq!(strip_rc_mention_prefix("  @rockai hello", "rockai"), "@rockai hello");
    }
}
