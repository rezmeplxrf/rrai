use super::{reply, BotData};
use crate::db::types::SessionStatus;
use serenity::all::*;
use std::sync::Arc;
use uuid::Uuid;

pub fn register() -> CreateCommand {
    CreateCommand::new("force")
        .description("Force-stop current session, clear queue, and send a new message")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "message",
                "Message to send after reset",
            )
            .required(true),
        )
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let channel_id_str = cmd.channel_id.to_string();

    // Force stop everything
    data.session_manager.stop_session(&channel_id_str).await;
    data.session_manager.clear_queue(&channel_id_str);
    data.db.upsert_session(
        &Uuid::new_v4().to_string(),
        &channel_id_str,
        None,
        SessionStatus::Idle,
    );

    let message = cmd
        .data
        .options
        .first()
        .and_then(|o| o.value.as_str())
        .map(|s| s.to_string());

    match message {
        Some(msg) => {
            reply(ctx, cmd, "🔄 Session reset. Processing new message...").await?;

            // Send the new message
            let sm = data.session_manager.clone();
            let channel_id = cmd.channel_id;
            let guild_id = cmd.guild_id.unwrap_or(GuildId::new(0));
            tokio::spawn(async move {
                if let Err(e) = sm.send_message(channel_id, guild_id, &msg).await {
                    tracing::warn!("Force message error: {e}");
                }
            });
            Ok(())
        }
        None => {
            reply(
                ctx,
                cmd,
                "🔄 Session force-stopped and queue cleared. Ready for new messages.",
            )
            .await
        }
    }
}
