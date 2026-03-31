use super::{reply, BotData};
use crate::bot::handlers::interaction::find_session_dir;
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("clear-sessions").description("Delete all session files for this project")
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
            if entry.file_name().to_string_lossy().ends_with(".jsonl") {
                if std::fs::remove_file(entry.path()).is_ok() {
                    count += 1;
                }
            }
        }
    }

    // Also clean up uploads directory
    let uploads_dir = std::path::Path::new(&project.project_path).join(".claude-uploads");
    if uploads_dir.exists() {
        let _ = std::fs::remove_dir_all(&uploads_dir);
    }

    // #13: Try to bulk-delete recent channel messages (up to 14 days old, Discord limit)
    let bulk_deleted = bulk_delete_channel_messages(ctx, cmd.channel_id).await;

    let mut msg = format!("🗑️ Deleted {count} session file(s).");
    if bulk_deleted > 0 {
        msg.push_str(&format!(" Cleared {bulk_deleted} message(s) from channel."));
    }
    msg.push_str(" Next message will start a new conversation.");

    reply(ctx, cmd, &msg).await
}

/// Bulk delete messages in the channel (up to 100, max 14 days old).
async fn bulk_delete_channel_messages(ctx: &Context, channel_id: ChannelId) -> usize {
    // Fetch recent messages
    let messages = match channel_id
        .messages(&ctx.http, GetMessages::new().limit(100))
        .await
    {
        Ok(msgs) => msgs,
        Err(_) => return 0,
    };

    if messages.is_empty() {
        return 0;
    }

    // Filter to messages less than 14 days old (Discord bulk-delete limit)
    let fourteen_days_ago = chrono::Utc::now() - chrono::Duration::days(14);
    let message_ids: Vec<MessageId> = messages
        .iter()
        .filter(|m| {
            let created = m.id.created_at();
            created.unix_timestamp() > fourteen_days_ago.timestamp()
        })
        .map(|m| m.id)
        .collect();

    if message_ids.is_empty() {
        return 0;
    }

    let count = message_ids.len();
    if count == 1 {
        // Single message — delete directly
        let _ = channel_id.delete_message(&ctx.http, message_ids[0]).await;
    } else {
        // Bulk delete
        let _ = channel_id
            .delete_messages(&ctx.http, &message_ids)
            .await;
    }
    count
}
