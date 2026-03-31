use super::{BotData, reply};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("auto-approve")
        .description("Toggle auto-approve mode for tool use")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "mode",
                "Enable or disable auto-approve",
            )
            .required(true)
            .add_string_choice("on", "on")
            .add_string_choice("off", "off"),
        )
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let mode = cmd
        .data
        .options
        .first()
        .and_then(|o| o.value.as_str())
        .unwrap_or("off");

    let channel_id_str = cmd.channel_id.to_string();
    let enable = mode == "on";
    if let Err(e) = data.db.set_auto_approve(&channel_id_str, enable) {
        tracing::warn!("Failed to set auto_approve: {e}");
    }

    if enable {
        reply(ctx, cmd, "⚡ Auto-approve **enabled** for this channel. All tool uses will be automatically approved.").await
    } else {
        reply(ctx, cmd, "🔒 Auto-approve **disabled** for this channel. Tool uses will require manual approval.").await
    }
}
