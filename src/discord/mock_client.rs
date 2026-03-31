use super::DiscordClient;
use parking_lot::Mutex;
use serenity::all::{ChannelId, CreateMessage, EditMessage, MessageId};
use std::sync::atomic::{AtomicU64, Ordering};

/// Recorded Discord API call for test assertions.
#[derive(Debug, Clone)]
pub enum DiscordCall {
    SendMessage {
        channel_id: ChannelId,
        // We can't inspect CreateMessage internals easily,
        // but we track that the call was made.
    },
    EditMessage {
        channel_id: ChannelId,
        message_id: MessageId,
    },
    GetChannelName {
        channel_id: ChannelId,
    },
}

/// Mock implementation of [`DiscordClient`] that records calls for test assertions.
///
/// Messages are assigned auto-incrementing IDs starting from 1.
/// Channel name lookups return a configurable default.
pub struct MockDiscordClient {
    calls: Mutex<Vec<DiscordCall>>,
    next_id: AtomicU64,
    channel_name: Mutex<Option<String>>,
    /// If set, send_message will return this error.
    send_error: Mutex<Option<String>>,
}

impl MockDiscordClient {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            channel_name: Mutex::new(None),
            send_error: Mutex::new(None),
        }
    }

    /// Set the name returned by `get_channel_name`.
    pub fn set_channel_name(&self, name: &str) {
        *self.channel_name.lock() = Some(name.to_string());
    }

    /// Make `send_message` return an error on next call.
    pub fn set_send_error(&self, error: &str) {
        *self.send_error.lock() = Some(error.to_string());
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<DiscordCall> {
        self.calls.lock().clone()
    }

    /// Count calls of a specific type.
    pub fn count_sends(&self) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|c| matches!(c, DiscordCall::SendMessage { .. }))
            .count()
    }

    pub fn count_edits(&self) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|c| matches!(c, DiscordCall::EditMessage { .. }))
            .count()
    }

    /// Clear recorded calls.
    pub fn clear(&self) {
        self.calls.lock().clear();
    }
}

impl Default for MockDiscordClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl DiscordClient for MockDiscordClient {
    async fn send_message(
        &self,
        channel_id: ChannelId,
        _message: CreateMessage,
    ) -> Result<MessageId, String> {
        if let Some(err) = self.send_error.lock().take() {
            return Err(err);
        }
        self.calls
            .lock()
            .push(DiscordCall::SendMessage { channel_id });
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        Ok(MessageId::new(id))
    }

    async fn edit_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
        _message: EditMessage,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .push(DiscordCall::EditMessage { channel_id, message_id });
        Ok(())
    }

    async fn get_channel_name(&self, channel_id: ChannelId) -> Option<String> {
        self.calls
            .lock()
            .push(DiscordCall::GetChannelName { channel_id });
        self.channel_name.lock().clone()
    }
}
