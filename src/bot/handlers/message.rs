use crate::claude::session_manager::SessionManager;
use crate::config::get_config;
use crate::db::Database;
use crate::security::{check_rate_limit, is_allowed_user};
use serenity::all::{ButtonStyle, Context, CreateActionRow, CreateButton, CreateMessage, Message};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

const MAX_FILE_SIZE: u64 = 25 * 1024 * 1024; // 25MB

static IMAGE_EXTENSIONS: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp"];
static BLOCKED_EXTENSIONS: &[&str] = &[
    ".exe", ".bat", ".cmd", ".com", ".msi", ".scr", ".pif", ".dll", ".sys", ".drv", ".vbs", ".vbe",
    ".wsf", ".wsh",
];

pub async fn handle_message(
    ctx: &Context,
    msg: &Message,
    db: &Database,
    session_manager: &Arc<SessionManager>,
) {
    // Ignore bots and DMs
    if msg.author.bot || msg.guild_id.is_none() {
        return;
    }

    let guild_id = msg.guild_id.unwrap();

    // Auth check
    if !is_allowed_user(msg.author.id.get()) {
        return;
    }

    // Rate limit
    if !check_rate_limit(msg.author.id.get()) {
        let _ = msg
            .reply(&ctx.http, "Rate limit exceeded. Please wait a moment.")
            .await;
        return;
    }

    let channel_id_str = msg.channel_id.to_string();

    // Check for pending custom text input
    if session_manager.has_pending_custom_input(&channel_id_str) {
        let text = msg.content.trim();
        if !text.is_empty() {
            session_manager.resolve_custom_input(&channel_id_str, text);
            let _ = msg.react(&ctx.http, '✅').await;
        }
        return;
    }

    let mut prompt = msg.content.trim().to_string();

    // Resolve project path (may not be registered yet — send_message will auto-register)
    let config = get_config();
    let project_path = match db.get_project(&channel_id_str) {
        Some(p) => p.project_path,
        None => config
            .sessions_dir()
            .join(&channel_id_str)
            .to_string_lossy()
            .to_string(),
    };

    // Download attachments
    let mut image_paths = Vec::new();
    let mut file_paths = Vec::new();
    let mut skipped = Vec::new();

    for attachment in &msg.attachments {
        let name = &attachment.filename;
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();

        if BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
            skipped.push(format!("Blocked: `{name}` (dangerous file type)"));
            continue;
        }

        if u64::from(attachment.size) > MAX_FILE_SIZE {
            let size_mb = attachment.size as f64 / 1024.0 / 1024.0;
            skipped.push(format!(
                "Skipped: `{name}` ({size_mb:.1}MB exceeds 25MB limit)"
            ));
            continue;
        }

        let upload_dir = Path::new(&project_path).join(".claude-uploads");
        tokio::fs::create_dir_all(&upload_dir).await.ok();

        let safe_name = name.replace(['/', '\\'], "_").replace("..", "_");
        let file_name = format!("{}-{safe_name}", chrono::Utc::now().timestamp_millis());
        let file_path = upload_dir.join(&file_name);

        // Verify path stays within upload dir
        let resolved = match file_path.canonicalize().or_else(|_| {
            // File doesn't exist yet, canonicalize parent
            upload_dir.canonicalize().map(|p| p.join(&file_name))
        }) {
            Ok(p) => p,
            Err(_) => {
                skipped.push(format!("Blocked: `{name}` (invalid filename)"));
                continue;
            }
        };
        let upload_canonical = upload_dir
            .canonicalize()
            .unwrap_or_else(|_| upload_dir.clone());
        if !resolved.starts_with(&upload_canonical) {
            skipped.push(format!("Blocked: `{name}` (invalid filename)"));
            continue;
        }

        // Download the file
        match reqwest::get(&attachment.url).await {
            Ok(response) if response.status().is_success() => match response.bytes().await {
                Ok(bytes) => {
                    if let Err(e) = tokio::fs::write(&file_path, &bytes).await {
                        warn!("Failed to write attachment {name}: {e}");
                        skipped.push(format!("Failed to download: `{name}`"));
                        continue;
                    }
                }
                Err(_) => {
                    skipped.push(format!("Failed to download: `{name}`"));
                    continue;
                }
            },
            _ => {
                skipped.push(format!("Failed to download: `{name}`"));
                continue;
            }
        }

        let path_str = file_path.to_string_lossy().to_string();
        if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            image_paths.push(path_str);
        } else {
            file_paths.push(path_str);
        }
    }

    if !skipped.is_empty() {
        let _ = msg.reply(&ctx.http, skipped.join("\n")).await;
    }

    if !image_paths.is_empty() {
        prompt.push_str("\n\n[Attached images - use Read tool to view these files]\n");
        prompt.push_str(&image_paths.join("\n"));
    }
    if !file_paths.is_empty() {
        prompt.push_str("\n\n[Attached files - use Read tool to read these files]\n");
        prompt.push_str(&file_paths.join("\n"));
    }

    if prompt.is_empty() {
        return;
    }

    // If session is busy, offer to queue
    if session_manager.is_busy(&channel_id_str).await {
        if session_manager.has_pending_queue(&channel_id_str) {
            let _ = msg
                .reply(
                    &ctx.http,
                    "⏳ A message is already waiting to be queued. Please press the button first.",
                )
                .await;
            return;
        }
        if session_manager.is_queue_full(&channel_id_str) {
            let _ = msg
                .reply(
                    &ctx.http,
                    "⏳ Queue is full (max 5). Please wait for the current task to finish.",
                )
                .await;
            return;
        }

        session_manager.set_pending_queue(&channel_id_str, msg.channel_id, guild_id, &prompt);

        let row = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("queue-yes:{}", channel_id_str))
                .label("Add to Queue")
                .style(ButtonStyle::Success)
                .emoji('✅'),
            CreateButton::new(format!("queue-no:{}", channel_id_str))
                .label("Cancel")
                .style(ButtonStyle::Secondary)
                .emoji('❌'),
        ]);

        let _ = msg
            .channel_id
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .content(
                        "⏳ A previous task is in progress. Process this automatically when done?",
                    )
                    .components(vec![row])
                    .reference_message(msg),
            )
            .await;
        return;
    }

    // Send message to Claude session
    let sm = session_manager.clone();
    let channel_id = msg.channel_id;
    let prompt_owned = prompt;
    tokio::spawn(async move {
        if let Err(e) = sm.send_message(channel_id, guild_id, &prompt_owned).await {
            warn!("sendMessage error: {e}");
            let _ = sm
                .discord()
                .send_message(channel_id, CreateMessage::new().content(format!("❌ {e}")))
                .await;
        }
    });
}
