use super::{reply_embed, BotData};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("models").description("Show available Claude models")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    // Common Claude models
    let models = [
        ("claude-opus-4-6", "Most capable, best for complex tasks"),
        ("claude-sonnet-4-6", "Balanced speed and quality"),
        ("claude-haiku-4-5-20251001", "Fastest, best for simple tasks"),
    ];

    let list: String = models
        .iter()
        .map(|(name, desc)| format!("• `{name}` — {desc}"))
        .collect::<Vec<_>>()
        .join("\n");

    let channel_id_str = cmd.channel_id.to_string();
    let current = data
        .db
        .get_model(&channel_id_str)
        .unwrap_or_else(|| "default".to_string());

    let embed = CreateEmbed::new()
        .title("🤖 Available Models")
        .description(format!("{list}\n\nCurrent: `{current}`\nUse `/model <name>` to change."))
        .color(0x5865f2);

    reply_embed(ctx, cmd, embed).await
}
