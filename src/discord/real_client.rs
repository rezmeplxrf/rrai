use super::DiscordClient;
use serenity::all::{ChannelId, CreateMessage, EditMessage, MessageId};
use serenity::http::Http;
use std::sync::Arc;

/// Production implementation of [`DiscordClient`] backed by serenity's HTTP client.
pub struct SerenityDiscordClient {
    http: Arc<Http>,
}

impl SerenityDiscordClient {
    pub fn new(http: Arc<Http>) -> Self {
        Self { http }
    }

    pub fn http(&self) -> &Http {
        &self.http
    }
}

#[async_trait::async_trait]
impl DiscordClient for SerenityDiscordClient {
    async fn send_message(
        &self,
        channel_id: ChannelId,
        message: CreateMessage,
    ) -> Result<MessageId, String> {
        let msg = channel_id
            .send_message(&self.http, message)
            .await
            .map_err(|e| format!("Failed to send message: {e}"))?;
        Ok(msg.id)
    }

    async fn edit_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
        message: EditMessage,
    ) -> Result<(), String> {
        channel_id
            .edit_message(&self.http, message_id, message)
            .await
            .map_err(|e| format!("Failed to edit message: {e}"))?;
        Ok(())
    }

    async fn get_channel_name(&self, channel_id: ChannelId) -> Option<String> {
        match self.http.get_channel(channel_id).await {
            Ok(ch) => ch.guild().map(|gc| gc.name.clone()),
            Err(_) => None,
        }
    }
}
