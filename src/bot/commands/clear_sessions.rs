use super::{reply, BotData};
use crate::bot::handlers::interaction::find_session_dir;
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("clear").description("Delete all session files and messages for this project")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let channel_id_str = cmd.channel_id.to_string();
    let project = match data.db.get_project(&channel_id_str) {
        Some(p) => p,
        None => return reply(ctx, cmd, "No project registered for this channel.").await,
    };

    // Stop any active session first
    data.session_manager.stop_session(&channel_id_str).await;
    data.db.clear_session(&channel_id_str);

    let session_dir = match find_session_dir(&project.project_path) {
        Some(d) => d,
        None => return reply(ctx, cmd, "No sessions found.").await,
    };

    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(&session_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.file_name().to_string_lossy().ends_with(".jsonl")
                && std::fs::remove_file(entry.path()).is_ok()
            {
                count += 1;
            }
        }
    }

    // Clean up uploads directory
    let uploads_dir = std::path::Path::new(&project.project_path).join(".claude-uploads");
    if uploads_dir.exists() {
        let _ = std::fs::remove_dir_all(&uploads_dir);
    }

    // Get the deferred reply message ID so we can exclude it from bulk delete
    let reply_msg = cmd.get_response(&ctx.http).await.ok();
    let reply_id = reply_msg.as_ref().map(|m| m.id);

    // Bulk-delete channel messages in a loop (Discord limits to 100 per call, 14 day max)
    let bulk_deleted = bulk_delete_channel_messages(ctx, cmd.channel_id, reply_id).await;

    let mut msg = format!("🗑️ Deleted {count} session file(s).");
    if bulk_deleted > 0 {
        msg.push_str(&format!(" Cleared {bulk_deleted} message(s) from channel."));
    }
    msg.push_str(" Next message will start a new conversation.");

    reply(ctx, cmd, &msg).await
}

/// Bulk delete messages in the channel, looping until all eligible messages are removed.
/// Excludes `exclude_id` (the bot's reply) from deletion.
async fn bulk_delete_channel_messages(
    ctx: &Context,
    channel_id: ChannelId,
    exclude_id: Option<MessageId>,
) -> usize {
    let fourteen_days_ago = chrono::Utc::now() - chrono::Duration::days(14);
    let mut total_deleted = 0;

    loop {
        let messages = match channel_id
            .messages(&ctx.http, GetMessages::new().limit(100))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => break,
        };

        if messages.is_empty() {
            break;
        }

        // Filter: < 14 days old, not the bot's reply
        let message_ids: Vec<MessageId> = messages
            .iter()
            .filter(|m| {
                if Some(m.id) == exclude_id {
                    return false;
                }
                let created = m.id.created_at();
                created.unix_timestamp() > fourteen_days_ago.timestamp()
            })
            .map(|m| m.id)
            .collect();

        if message_ids.is_empty() {
            break;
        }

        let batch_count = message_ids.len();
        if batch_count == 1 {
            let _ = channel_id.delete_message(&ctx.http, message_ids[0]).await;
        } else {
            let _ = channel_id
                .delete_messages(&ctx.http, &message_ids)
                .await;
        }

        total_deleted += batch_count;

        // If we got fewer than 100, we've exhausted the eligible messages
        if messages.len() < 100 {
            break;
        }
    }

    total_deleted
}
