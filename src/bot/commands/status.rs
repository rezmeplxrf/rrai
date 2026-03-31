use super::{reply_embed, BotData};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("status").description("Show all project session statuses")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let guild_id = cmd.guild_id.map(|g| g.to_string()).unwrap_or_default();
    let sessions = data.db.get_all_sessions(&guild_id);

    if sessions.is_empty() {
        let embed = CreateEmbed::new()
            .title("📊 Bot Status")
            .description("No projects registered yet.\nSend a message in any channel to auto-register.")
            .color(0x5865f2);
        return reply_embed(ctx, cmd, embed).await;
    }

    let mut lines = Vec::new();
    for s in &sessions {
        let status_icon = match s.session.status {
            crate::db::types::SessionStatus::Online => "🟢",
            crate::db::types::SessionStatus::Waiting => "🟡",
            crate::db::types::SessionStatus::Idle => "🔵",
            crate::db::types::SessionStatus::Offline => "⚫",
        };
        let channel_id: u64 = s.session.channel_id.parse().unwrap_or(0);
        let path = s.project_path.rsplit('/').next().unwrap_or(&s.project_path);
        lines.push(format!(
            "{status_icon} <#{}> — `{}` — **{}**",
            channel_id,
            path,
            s.session.status.as_str()
        ));
    }

    let embed = CreateEmbed::new()
        .title("📊 Bot Status")
        .description(lines.join("\n"))
        .color(0x5865f2)
        .timestamp(Timestamp::now());

    reply_embed(ctx, cmd, embed).await
}
