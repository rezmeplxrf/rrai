use super::{reply, reply_embed, BotData};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("context").description("Show context window usage")
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let channel_id_str = cmd.channel_id.to_string();
    let project = match data.db.get_project(&channel_id_str) {
        Some(p) => p,
        None => return reply(ctx, cmd, "No project registered for this channel.").await,
    };

    // Check for CLAUDE.md
    let claude_md_path = std::path::Path::new(&project.project_path).join("CLAUDE.md");
    let claude_md_info = if claude_md_path.exists() {
        match std::fs::metadata(&claude_md_path) {
            Ok(m) => format!("**CLAUDE.md:** {} bytes", m.len()),
            Err(_) => "**CLAUDE.md:** exists (unreadable)".to_string(),
        }
    } else {
        "**CLAUDE.md:** not found".to_string()
    };

    // Check session status
    let session = data.db.get_session(&channel_id_str);
    let session_info = match &session {
        Some(s) => {
            let status = s.status.as_str();
            let sid = s.session_id.as_deref().unwrap_or("none");
            format!(
                "**Session:** `{}...` ({})",
                &sid[..8.min(sid.len())],
                status
            )
        }
        None => "**Session:** none".to_string(),
    };

    let desc = format!(
        "**Project:** `{}`\n{claude_md_info}\n{session_info}\n\n\
         *Note: Detailed context window usage (token breakdown, categories) \
         requires querying the live Claude session via the Agent SDK.*",
        project.project_path
    );

    let embed = CreateEmbed::new()
        .title("📝 Context")
        .description(desc)
        .color(0x5865f2)
        .timestamp(Timestamp::now());

    reply_embed(ctx, cmd, embed).await
}
