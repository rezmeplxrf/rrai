use super::{reply, reply_embed_with_components, BotData};
use crate::bot::handlers::interaction::find_session_dir;
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("sessions").description("List and manage existing Claude sessions")
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

    let session_dir = match find_session_dir(&project.project_path) {
        Some(d) => d,
        None => return reply(ctx, cmd, "No sessions found.").await,
    };

    // List .jsonl files, filtering out small/empty sessions (#27)
    let mut session_files: Vec<(String, std::time::SystemTime, Option<String>)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&session_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".jsonl") {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            // Skip tiny sessions (< 512 bytes)
            if meta.len() < 512 {
                continue;
            }
            let session_id = name.trim_end_matches(".jsonl").to_string();
            let modified = meta.modified().ok();

            // #28: Read first user message for label
            let first_msg = read_first_user_message(&entry.path());

            // Skip sessions with no user message
            if first_msg.is_none() {
                continue;
            }

            session_files.push((
                session_id,
                modified.unwrap_or(std::time::UNIX_EPOCH),
                first_msg,
            ));
        }
    }

    if session_files.is_empty() {
        return reply(ctx, cmd, "No sessions found.").await;
    }

    // Sort by most recent
    session_files.sort_by(|a, b| b.1.cmp(&a.1));

    // Take the 24 most recent (Discord select menu limit = 25, minus "New Session")
    session_files.truncate(24);

    let mut options: Vec<CreateSelectMenuOption> = vec![
        CreateSelectMenuOption::new("✨ New Session", "__new_session__")
            .description("Start a fresh conversation"),
    ];

    let current_session = data.db.get_session(&channel_id_str);
    let current_sid = current_session.and_then(|s| s.session_id);

    for (sid, modified, first_msg) in &session_files {
        let elapsed = modified
            .elapsed()
            .map(|d| format_elapsed(d.as_secs()))
            .unwrap_or_else(|_| "unknown".to_string());

        // #28: Show first user message as label (truncated to 50 chars)
        let msg_preview = first_msg
            .as_deref()
            .unwrap_or("(no message)");
        let label = if msg_preview.len() > 50 {
            format!("{}...", crate::claude::output_formatter::truncate(msg_preview, 47))
        } else {
            msg_preview.to_string()
        };

        let is_current = current_sid.as_deref() == Some(sid.as_str());
        let desc_text = if is_current {
            format!("Current • {elapsed}")
        } else {
            elapsed
        };

        let mut opt = CreateSelectMenuOption::new(label, sid);
        opt = opt.description(desc_text);
        if is_current {
            opt = opt.default_selection(true);
        }
        options.push(opt);
    }

    let select = CreateSelectMenu::new(
        "session-select",
        CreateSelectMenuKind::String { options },
    )
    .placeholder("Select a session...");

    let embed = CreateEmbed::new()
        .title(format!("📋 Sessions ({})", session_files.len()))
        .description("Select a session to resume or delete.")
        .color(0x7c3aed);

    reply_embed_with_components(
        ctx,
        cmd,
        embed,
        vec![CreateActionRow::SelectMenu(select)],
    )
    .await
}

/// Read the first user message from a JSONL session file.
/// Strips IDE-injected tags (e.g. `<ide_opened_file>...</ide_opened_file>`).
fn read_first_user_message(file_path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(file_path).ok()?;
    for line in content.lines() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) != Some("user") {
                continue;
            }
            if let Some(msg) = val.get("message") {
                // SDK format: message.content (string)
                if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                    let cleaned = strip_tags(text);
                    if !cleaned.is_empty() {
                        return Some(cleaned);
                    }
                }
                // Array format: message.content[].text
                if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                    for block in blocks {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            let cleaned = strip_tags(text);
                            if !cleaned.is_empty() {
                                return Some(cleaned);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Strip XML-like tags injected by IDEs (e.g. `<ide_opened_file>...</ide_opened_file>`,
/// `<system-reminder>...`). Matches TS: `raw.replace(/<[^>]+>[^<]*<\/[^>]+>/g, "").replace(/<[^>]+>/g, "").trim()`
fn strip_tags(text: &str) -> String {
    // First remove paired tags with content: <tag>content</tag>
    let re_paired = regex::Regex::new(r"<[^>]+>[^<]*</[^>]+>").unwrap();
    let result = re_paired.replace_all(text, "");
    // Then remove remaining standalone tags: <tag>
    let re_single = regex::Regex::new(r"<[^>]+>").unwrap();
    let result = re_single.replace_all(&result, "");
    result.trim().to_string()
}

fn format_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        "just now".to_string()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}
