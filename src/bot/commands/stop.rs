use super::{BotData, reply};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("stop").description("Stop the active Claude session")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let channel_id_str = cmd.channel_id.to_string();
    let stopped = data.session_manager.stop_session(&channel_id_str).await;

    if stopped {
        reply(ctx, cmd, "⏹️ Session has been stopped.").await
    } else {
        reply(ctx, cmd, "No active session in this channel.").await
    }
}
