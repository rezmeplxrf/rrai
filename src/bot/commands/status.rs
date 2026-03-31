use super::{BotData, reply_embed};
use crate::db::types::SessionStatus;
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
    let projects = data.db.get_all_projects(&guild_id);

    if projects.is_empty() {
        let embed = CreateEmbed::new()
            .title("📊 Bot Status")
            .description(
                "No projects registered yet.\nSend a message in any channel to auto-register.",
            )
            .color(0x5865f2);
        return reply_embed(ctx, cmd, embed).await;
    }

    let mut lines = Vec::new();
    for project in &projects {
        // Get session status for this project (may not exist)
        let session = data.db.get_session(&project.channel_id);
        let status = session
            .as_ref()
            .map(|s| s.status)
            .unwrap_or(SessionStatus::Offline);

        let status_icon = match status {
            SessionStatus::Online => "🟢",
            SessionStatus::Waiting => "🟡",
            SessionStatus::Idle => "⚪",
            SessionStatus::Offline => "🔴",
        };
        let channel_id: u64 = project.channel_id.parse().unwrap_or(0);
        let path = project
            .project_path
            .rsplit('/')
            .next()
            .unwrap_or(&project.project_path);
        let auto = if project.auto_approve { " ⚡" } else { "" };
        lines.push(format!(
            "{status_icon} <#{channel_id}> — `{path}` — **{}**{auto}",
            status.as_str()
        ));
    }

    let embed = CreateEmbed::new()
        .title("📊 Bot Status")
        .description(lines.join("\n"))
        .color(0x5865f2)
        .timestamp(Timestamp::now());

    reply_embed(ctx, cmd, embed).await
}
