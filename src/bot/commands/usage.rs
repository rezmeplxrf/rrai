use super::{BotData, reply_embed};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("usage").description("Show Claude Code API usage information")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    _data: &Arc<BotData>,
) -> Result<(), String> {
    // Usage info would require OAuth token refresh + API calls to claude.ai
    // For now, show a placeholder directing users to the web dashboard
    let embed = CreateEmbed::new()
        .title("📊 Usage")
        .description(
            "Usage tracking requires Claude API access.\n\n\
             Check your usage at: https://claude.ai/settings/usage\n\n\
             *Note: Detailed usage dashboard integration is planned for a future release.*",
        )
        .color(0x5865f2)
        .timestamp(Timestamp::now());

    reply_embed(ctx, cmd, embed).await
}
