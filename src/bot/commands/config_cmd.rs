use super::{reply_embed, BotData};
use crate::config::get_config;
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("config").description("Show current bot configuration")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    _data: &Arc<BotData>,
) -> Result<(), String> {
    let config = get_config();

    let token_preview = format!("{}***", &config.discord_bot_token[..4.min(config.discord_bot_token.len())]);

    let user_ids: String = config
        .allowed_user_ids
        .iter()
        .map(|id| format!("`{id}`"))
        .collect::<Vec<_>>()
        .join(", ");

    let embed = CreateEmbed::new()
        .title("⚙️ Configuration")
        .field("Token", format!("`{token_preview}`"), false)
        .field("Guild ID", format!("`{}`", config.discord_guild_id), true)
        .field("Allowed Users", user_ids, false)
        .field(
            "Data Dir",
            format!("`{}`", config.data_dir),
            false,
        )
        .field(
            "Rate Limit",
            format!("{}/min", config.rate_limit_per_minute),
            true,
        )
        .color(0x5865f2)
        .timestamp(Timestamp::now());

    reply_embed(ctx, cmd, embed).await
}
