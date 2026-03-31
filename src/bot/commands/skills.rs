use super::{reply_embed, BotData};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("skills").description("Show available Claude Code skills")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    _data: &Arc<BotData>,
) -> Result<(), String> {
    let embed = CreateEmbed::new()
        .title("🎯 Available Skills")
        .description(
            "Skills are Claude Code slash commands available in sessions:\n\n\
             • `/commit` — Create a git commit\n\
             • `/review-pr` — Review a pull request\n\
             • `/help` — Get help\n\
             • `/clear` — Clear conversation\n\
             • `/compact` — Compact conversation\n\
             • `/init` — Initialize CLAUDE.md\n\n\
             *Skills are used within Claude sessions, not as Discord commands.*",
        )
        .color(0x5865f2);

    reply_embed(ctx, cmd, embed).await
}
