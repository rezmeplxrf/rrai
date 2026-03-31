use super::{reply_embed, BotData};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("agents").description("Show available Claude Code agent types")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    _data: &Arc<BotData>,
) -> Result<(), String> {
    let embed = CreateEmbed::new()
        .title("🤖 Available Agents")
        .description(
            "Claude Code supports specialized agent types:\n\n\
             • **general-purpose** — Default agent for complex tasks\n\
             • **Explore** — Fast codebase exploration\n\
             • **Plan** — Software architecture planning\n\n\
             *Agents are used within Claude sessions via the Agent tool.*",
        )
        .color(0x5865f2);

    reply_embed(ctx, cmd, embed).await
}
