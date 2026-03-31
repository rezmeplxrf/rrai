use super::{BotData, reply};
use crate::bot::handlers::interaction::{find_session_dir, get_last_assistant_message};
use crate::claude::output_formatter::split_message;
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("last").description("Show the last Claude response from this session")
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

    let session = match data.db.get_session(&channel_id_str) {
        Some(s) => s,
        None => return reply(ctx, cmd, "No active session.").await,
    };

    let session_id = match &session.claude_session_id {
        Some(s) => s,
        None => return reply(ctx, cmd, "No session history available.").await,
    };

    let session_dir = match find_session_dir(&project.project_path) {
        Some(d) => d,
        None => return reply(ctx, cmd, "Session directory not found.").await,
    };

    let file_path = session_dir.join(format!("{session_id}.jsonl"));
    let last_msg = get_last_assistant_message(&file_path);

    match last_msg {
        Some(msg) if !msg.is_empty() && msg != "(no message)" => {
            // Use splitMessage to properly handle long responses with code fences
            let chunks = split_message(&msg);
            if let Some(first) = chunks.first() {
                reply(ctx, cmd, first).await?;
            }
            // Send overflow chunks as follow-up messages
            for chunk in chunks.iter().skip(1) {
                let _ = cmd
                    .channel_id
                    .send_message(&ctx.http, CreateMessage::new().content(chunk))
                    .await;
            }
            Ok(())
        }
        _ => reply(ctx, cmd, "No response found in current session.").await,
    }
}
