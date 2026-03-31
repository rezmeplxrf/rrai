use crate::claude::session_manager::SessionManager;
use crate::db::Database;
use crate::db::types::SessionStatus;
use crate::security::is_allowed_user;
use serenity::all::{
    ButtonStyle, ComponentInteraction, ComponentInteractionDataKind, Context, CreateActionRow,
    CreateButton, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse,
};
use std::sync::Arc;
use uuid::Uuid;

pub async fn handle_button_interaction(
    ctx: &Context,
    interaction: &ComponentInteraction,
    db: &Database,
    session_manager: &Arc<SessionManager>,
) {
    if !is_allowed_user(interaction.user.id.get()) {
        let _ = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("You are not authorized.")
                        .ephemeral(true),
                ),
            )
            .await;
        return;
    }

    let custom_id = &interaction.data.custom_id;
    let (action, request_id) = match custom_id.find(':') {
        Some(idx) => (&custom_id[..idx], &custom_id[idx + 1..]),
        None => (custom_id.as_str(), ""),
    };

    if request_id.is_empty() {
        let _ = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("Invalid button interaction.")
                        .ephemeral(true),
                ),
            )
            .await;
        return;
    }

    match action {
        "stop" => {
            let channel_id = request_id;
            let stopped = session_manager.stop_session(channel_id).await;
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content("⏹️ Task has been stopped.")
                            .components(vec![]),
                    ),
                )
                .await;
            if !stopped {
                let _ = interaction
                    .create_followup(
                        &ctx.http,
                        serenity::all::CreateInteractionResponseFollowup::new()
                            .content("No active session.")
                            .ephemeral(true),
                    )
                    .await;
            }
        }

        "queue-yes" => {
            let channel_id = request_id;
            let confirmed = session_manager.confirm_queue(channel_id);
            let content = if confirmed {
                let size = session_manager.get_queue_size(channel_id);
                format!(
                    "📨 Message added to queue ({size}/5). It will be processed after the current task."
                )
            } else {
                "⏳ Queue request has expired.".to_string()
            };
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content(content)
                            .components(vec![]),
                    ),
                )
                .await;
        }

        "queue-no" => {
            session_manager.cancel_queue(request_id);
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content("Cancelled.")
                            .components(vec![]),
                    ),
                )
                .await;
        }

        "session-resume" => {
            let session_id = request_id;
            let channel_id = interaction.channel_id.to_string();
            session_manager.stop_session(&channel_id).await;
            if let Err(e) = db.upsert_session(
                &Uuid::new_v4().to_string(),
                &channel_id,
                Some(session_id),
                SessionStatus::Idle,
            ) {
                tracing::warn!("Failed to upsert session: {e}");
            }
            let embed = CreateEmbed::new()
                .title("Session Resumed")
                .description(format!(
                    "Session: `{}...`\n\nNext message you send will resume this conversation.",
                    &session_id[..8.min(session_id.len())]
                ))
                .color(0x00ff00);
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .embed(embed)
                            .components(vec![]),
                    ),
                )
                .await;
        }

        "session-cancel" => {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content("Cancelled.")
                            .components(vec![]),
                    ),
                )
                .await;
        }

        "session-delete" => {
            let session_id = request_id;
            let channel_id_str = interaction.channel_id.to_string();
            let project = db.get_project(&channel_id_str);

            if let Some(project) = project {
                let session_dir = find_session_dir(&project.project_path);
                if let Some(dir) = session_dir {
                    let file_path = dir.join(format!("{session_id}.jsonl"));
                    match std::fs::remove_file(&file_path) {
                        Ok(_) => {
                            // If deleting the active session, reset DB
                            if let Some(db_session) = db.get_session(&channel_id_str)
                                && db_session.claude_session_id.as_deref() == Some(session_id)
                                && let Err(e) = db.upsert_session(
                                    &Uuid::new_v4().to_string(),
                                    &channel_id_str,
                                    None,
                                    SessionStatus::Idle,
                                )
                            {
                                tracing::warn!("Failed to upsert session: {e}");
                            }
                            let embed = CreateEmbed::new()
                                .title("Session Deleted")
                                .description(format!(
                                    "Session `{}...` has been deleted.\nYour next message will start a new conversation.",
                                    &session_id[..8.min(session_id.len())]
                                ))
                                .color(0xff6b6b);
                            let _ = interaction
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::UpdateMessage(
                                        CreateInteractionResponseMessage::new()
                                            .embed(embed)
                                            .components(vec![]),
                                    ),
                                )
                                .await;
                        }
                        Err(_) => {
                            let _ = interaction
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::UpdateMessage(
                                        CreateInteractionResponseMessage::new()
                                            .content("Failed to delete session file.")
                                            .components(vec![]),
                                    ),
                                )
                                .await;
                        }
                    }
                }
            }
        }

        "ask-opt" => {
            // request_id format: "uuid:optionIndex"
            let last_colon = request_id.rfind(':').unwrap_or(0);
            let actual_request_id = &request_id[..last_colon];

            // Get button label from the interaction
            let selected_label = interaction
                .data
                .custom_id
                .split(':')
                .next_back()
                .unwrap_or("Unknown");

            // Try to get label from message components
            let label = get_button_label(interaction, custom_id)
                .unwrap_or_else(|| selected_label.to_string());

            let resolved = session_manager.resolve_question(actual_request_id, &label);
            if !resolved {
                let _ = interaction
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("This question has expired.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }

            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content(format!("✅ Selected: **{label}**"))
                            .components(vec![]),
                    ),
                )
                .await;
        }

        "ask-other" => {
            session_manager.enable_custom_input(request_id, &interaction.channel_id.to_string());
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content("✏️ Type your answer...")
                            .components(vec![]),
                    ),
                )
                .await;
        }

        "queue-clear" => {
            let channel_id = request_id;
            let cleared = session_manager.clear_queue(channel_id);
            let embed = CreateEmbed::new()
                .title("Queue Cleared")
                .description(format!("Cleared {cleared} queued message(s)."))
                .color(0xff6600);
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .embed(embed)
                            .components(vec![]),
                    ),
                )
                .await;
        }

        "queue-remove" => {
            // request_id format: "channelId:index"
            let last_colon = request_id.rfind(':').unwrap_or(0);
            let channel_id = &request_id[..last_colon];
            let index: usize = request_id[last_colon + 1..].parse().unwrap_or(0);
            let removed = session_manager.remove_from_queue(channel_id, index);

            if let Some(removed_prompt) = removed {
                let preview = if removed_prompt.len() > 60 {
                    format!(
                        "{}…",
                        crate::claude::output_formatter::truncate(&removed_prompt, 60)
                    )
                } else {
                    removed_prompt.clone()
                };

                let queue = session_manager.get_queue_prompts(channel_id);
                if queue.is_empty() {
                    let embed = CreateEmbed::new()
                        .title("Message Removed")
                        .description(format!("Removed: {preview}\n\nQueue is now empty."))
                        .color(0xff6600);
                    let _ = interaction
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .embed(embed)
                                    .components(vec![]),
                            ),
                        )
                        .await;
                } else {
                    let list: String = queue
                        .iter()
                        .enumerate()
                        .map(|(idx, p)| {
                            let p_preview = if p.len() > 100 {
                                format!("{}…", crate::claude::output_formatter::truncate(p, 100))
                            } else {
                                p.clone()
                            };
                            format!("**{}.** {p_preview}", idx + 1)
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");

                    let mut rows: Vec<CreateActionRow> = Vec::new();
                    let mut buttons: Vec<CreateButton> = queue
                        .iter()
                        .enumerate()
                        .take(19)
                        .map(|(idx, _)| {
                            CreateButton::new(format!("queue-remove:{channel_id}:{idx}"))
                                .label(format!("❌ {}", idx + 1))
                                .style(ButtonStyle::Secondary)
                        })
                        .collect();
                    buttons.push(
                        CreateButton::new(format!("queue-clear:{channel_id}"))
                            .label("Clear All")
                            .style(ButtonStyle::Danger),
                    );

                    for chunk in buttons.chunks(5) {
                        rows.push(CreateActionRow::Buttons(chunk.to_vec()));
                    }

                    let embed = CreateEmbed::new()
                        .title(format!("📋 Message Queue ({})", queue.len()))
                        .description(format!("~~{preview}~~ removed\n\n{list}"))
                        .color(0x5865f2);
                    let _ = interaction
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .embed(embed)
                                    .components(rows),
                            ),
                        )
                        .await;
                }
            } else {
                let _ = interaction
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new()
                                .content("This item is no longer in the queue.")
                                .components(vec![]),
                        ),
                    )
                    .await;
            }
        }

        "approve" | "deny" | "approve-all" => {
            let resolved = session_manager.resolve_approval(request_id, action);
            if !resolved {
                let _ = interaction
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("This approval request has expired.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }

            let label = match action {
                "approve" => "✅ Approved",
                "deny" => "❌ Denied",
                "approve-all" => "⚡ Auto-approve enabled for this channel",
                _ => unreachable!(),
            };
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content(label)
                            .components(vec![]),
                    ),
                )
                .await;
        }

        _ => {}
    }
}

pub async fn handle_select_menu_interaction(
    ctx: &Context,
    interaction: &ComponentInteraction,
    db: &Database,
    session_manager: &Arc<SessionManager>,
) {
    if !is_allowed_user(interaction.user.id.get()) {
        let _ = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("You are not authorized.")
                        .ephemeral(true),
                ),
            )
            .await;
        return;
    }

    let custom_id = &interaction.data.custom_id;

    // Handle AskUserQuestion multi-select
    if let Some(ask_request_id) = custom_id.strip_prefix("ask-select:") {
        let selected_values = match &interaction.data.kind {
            ComponentInteractionDataKind::StringSelect { values } => values.clone(),
            _ => vec![],
        };
        // #24: Resolve value indices to option labels from the select menu component
        let answer = resolve_select_labels(interaction, &selected_values);

        let resolved = session_manager.resolve_question(ask_request_id, &answer);
        if !resolved {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("This question has expired.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let _ = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content(format!("✅ Selected: **{answer}**"))
                        .components(vec![]),
                ),
            )
            .await;
        return;
    }

    // Handle session select menu
    if custom_id == "session-select" {
        let selected = match &interaction.data.kind {
            ComponentInteractionDataKind::StringSelect { values } => match values.first() {
                Some(v) => v.clone(),
                None => return,
            },
            _ => return,
        };

        if selected == "__new_session__" {
            let channel_id_str = interaction.channel_id.to_string();
            session_manager.stop_session(&channel_id_str).await;
            if let Err(e) = db.upsert_session(
                &Uuid::new_v4().to_string(),
                &channel_id_str,
                None,
                SessionStatus::Idle,
            ) {
                tracing::warn!("Failed to upsert session: {e}");
            }

            let embed = CreateEmbed::new()
                .title("✨ New Session")
                .description(
                    "New session is ready.\nA new conversation will start from your next message.",
                )
                .color(0x00ff00);
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .embed(embed)
                            .components(vec![]),
                    ),
                )
                .await;
            return;
        }

        // Defer to avoid timeout while reading session files
        let _ = interaction
            .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
            .await;

        let channel_id_str = interaction.channel_id.to_string();
        let project = db.get_project(&channel_id_str);
        let mut last_message = String::new();
        if let Some(project) = &project
            && let Some(dir) = find_session_dir(&project.project_path)
        {
            let file_path = dir.join(format!("{selected}.jsonl"));
            last_message = get_last_assistant_message(&file_path).unwrap_or_default();
        }

        let row = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("session-resume:{selected}"))
                .label("Resume")
                .style(ButtonStyle::Success)
                .emoji('▶'),
            CreateButton::new(format!("session-delete:{selected}"))
                .label("Delete")
                .style(ButtonStyle::Danger)
                .emoji('🗑'),
            CreateButton::new("session-cancel:_")
                .label("Cancel")
                .style(ButtonStyle::Secondary),
        ]);

        let preview = if !last_message.is_empty() && last_message != "(no message)" {
            let truncated = if last_message.len() > 300 {
                format!(
                    "{}...",
                    crate::claude::output_formatter::truncate(&last_message, 300)
                )
            } else {
                last_message
            };
            format!("\n\n**Last conversation:**\n{truncated}")
        } else {
            String::new()
        };

        let embed = CreateEmbed::new()
            .title("Session Selected")
            .description(format!(
                "Session: `{}...`\n\nResume or delete this session?{preview}",
                &selected[..8.min(selected.len())]
            ))
            .color(0x7c3aed);

        let _ = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new()
                    .embed(embed)
                    .components(vec![row]),
            )
            .await;
    }
}

/// Find the Claude Code session directory for a project.
pub fn find_session_dir(project_path: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.exists() {
        return None;
    }

    // Claude Code encodes the project path: / and \ and _ become -
    let canonical = std::path::Path::new(project_path).canonicalize().ok()?;
    let canonical_str = canonical.to_string_lossy();

    // Try dash-encoded variant (replaces / \ _ with -)
    let dash_encoded: String = canonical_str
        .chars()
        .map(|c| match c {
            '/' | '\\' | '_' => '-',
            _ => c,
        })
        .collect();
    let dir = projects_dir.join(&dash_encoded);
    if dir.exists() {
        return Some(dir);
    }

    // Try percent-encoded variant
    let percent_encoded = canonical_str.replace('/', "%2F");
    let dir2 = projects_dir.join(&percent_encoded);
    if dir2.exists() {
        return Some(dir2);
    }

    // Fallback: scan directories, try decoding names and reading JSONL cwd fields
    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            // Try percent-decoding
            let decoded = name.replace("%2F", "/");
            if decoded == canonical_str.as_ref() {
                return Some(entry.path());
            }

            // Try reading a JSONL file's cwd field as final fallback
            if let Ok(files) = std::fs::read_dir(entry.path()) {
                for file in files.filter_map(|f| f.ok()) {
                    if !file.file_name().to_string_lossy().ends_with(".jsonl") {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(file.path())
                        && let Some(first_line) = content.lines().next()
                        && let Ok(val) = serde_json::from_str::<serde_json::Value>(first_line)
                        && let Some(cwd) = val.get("cwd").and_then(|c| c.as_str())
                        && cwd == canonical_str.as_ref()
                    {
                        return Some(entry.path());
                    }
                    break; // Only check the first JSONL file
                }
            }
        }
    }

    None
}

/// Read the last assistant message from a JSONL session file.
/// Concatenates ALL text blocks in the last assistant message (not just the first).
/// Handles both flat `{type:"assistant", content:[...]}` and nested
/// `{type:"assistant", message:{content:[...]}}` JSONL formats.
pub fn get_last_assistant_message(file_path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(file_path).ok()?;

    for line in content.lines().rev() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) != Some("assistant") {
                continue;
            }
            // Try flat format: content at top level
            // Then nested format: message.content
            let content_arr = val.get("content").and_then(|c| c.as_array()).or_else(|| {
                val.get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
            });

            if let Some(blocks) = content_arr {
                let mut full_text = String::new();
                for block in blocks {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str())
                        && !text.is_empty()
                    {
                        if !full_text.is_empty() {
                            full_text.push('\n');
                        }
                        full_text.push_str(text);
                    }
                }
                if !full_text.is_empty() {
                    return Some(full_text);
                }
            }
        }
    }

    None
}

/// #24: Resolve select menu value indices to their display labels.
fn resolve_select_labels(interaction: &ComponentInteraction, values: &[String]) -> String {
    // Try to find the select menu component and map value -> label
    for row in &interaction.message.components {
        for component in &row.components {
            if let serenity::model::application::ActionRowComponent::SelectMenu(menu) = component {
                let options = &menu.options;
                let labels: Vec<String> = values
                    .iter()
                    .map(|val| {
                        options
                            .iter()
                            .find(|opt| opt.value == *val)
                            .map(|opt| opt.label.clone())
                            .unwrap_or_else(|| val.clone())
                    })
                    .collect();
                return labels.join(", ");
            }
        }
    }
    // Fallback: use raw values
    values.join(", ")
}

fn get_button_label(interaction: &ComponentInteraction, custom_id: &str) -> Option<String> {
    for row in &interaction.message.components {
        for component in &row.components {
            if let serenity::model::application::ActionRowComponent::Button(btn) = component
                && let serenity::model::application::ButtonKind::NonLink {
                    custom_id: ref cid, ..
                } = btn.data
                && cid == custom_id
            {
                return btn.label.clone();
            }
        }
    }
    None
}
