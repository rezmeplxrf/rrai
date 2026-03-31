pub mod mock_client;
mod real_client;

pub use mock_client::MockDiscordClient;
pub use real_client::SerenityDiscordClient;

use serenity::all::{ChannelId, CreateMessage, EditMessage, MessageId};

/// Trait abstracting Discord HTTP operations for testability.
///
/// Production code uses [`SerenityDiscordClient`] (wraps `Arc<Http>`).
/// Tests use a mock implementation that records calls.
#[async_trait::async_trait]
pub trait DiscordClient: Send + Sync {
    /// Send a message to a channel. Returns the new message's ID.
    async fn send_message(
        &self,
        channel_id: ChannelId,
        message: CreateMessage,
    ) -> Result<MessageId, String>;

    /// Edit an existing message in a channel.
    async fn edit_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
        message: EditMessage,
    ) -> Result<(), String>;

    /// Get a channel's name, or None if unavailable.
    async fn get_channel_name(&self, channel_id: ChannelId) -> Option<String>;
}
