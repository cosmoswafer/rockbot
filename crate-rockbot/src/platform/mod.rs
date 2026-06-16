pub mod matrix;
pub mod rocketchat;

use std::future::Future;
use std::pin::Pin;

use async_trait::async_trait;

use crate::error::Result;

pub use self::matrix::MatrixPlatform;
pub use self::rocketchat::{RcPlatformSender, RocketChatPlatform};
pub use ::rocketchat::IncomingMessage;

#[async_trait]
pub trait PlatformSender: Send + Sync {
    async fn send_reply(&self, text: &str, alias: Option<&str>) -> Result<()>;

    async fn send_reply_with_attachments(
        &self,
        text: &str,
        attachments: &[serde_json::Value],
        alias: Option<&str>,
    ) -> Result<()>;

    async fn send_typing(&self, typing: bool) -> Result<()>;

    fn room_id(&self) -> &str;

    fn as_any(&self) -> &dyn std::any::Any;

    fn clone_box(&self) -> Box<dyn PlatformSender>;
}

impl Clone for Box<dyn PlatformSender> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub type MessageHandler = Box<
    dyn Fn(
            IncomingMessage,
            Box<dyn PlatformSender>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync
        + 'static,
>;

#[async_trait]
pub trait MessagingClient: Send + Sync {
    async fn connect_and_run(&self, handler: MessageHandler) -> Result<()>;
}
