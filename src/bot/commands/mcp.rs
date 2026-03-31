use super::{BotData, reply, reply_embed};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("mcp")
        .description("Show or toggle MCP servers")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "server",
                "MCP server name to toggle",
            )
            .required(false)
            .set_autocomplete(true),
        )
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let channel_id_str = cmd.channel_id.to_string();
    let server_name = cmd
        .data
        .options
        .first()
        .and_then(|o| o.value.as_str())
        .map(|s| s.to_string());

    match server_name {
        Some(name) => {
            let disabled = data.db.get_disabled_mcps(&channel_id_str);
            let currently_disabled = disabled.contains(&name);

            // Toggle via SDK if session is active
            if data.session_manager.is_active(&channel_id_str).await {
                data.session_manager
                    .toggle_mcp_server(&channel_id_str, &name, currently_disabled)
                    .await;
            } else {
                // No active session — just update DB
                if currently_disabled {
                    let updated: Vec<String> =
                        disabled.into_iter().filter(|n| n != &name).collect();
                    if let Err(e) = data.db.set_disabled_mcps(&channel_id_str, &updated) {
                        tracing::warn!("Failed to update disabled MCPs: {e}");
                    }
                } else {
                    let mut updated = disabled;
                    updated.push(name.clone());
                    if let Err(e) = data.db.set_disabled_mcps(&channel_id_str, &updated) {
                        tracing::warn!("Failed to update disabled MCPs: {e}");
                    }
                }
            }

            if currently_disabled {
                reply(ctx, cmd, &format!("✅ MCP server `{name}` **enabled**.")).await
            } else {
                reply(ctx, cmd, &format!("❌ MCP server `{name}` **disabled**.")).await
            }
        }
        None => {
            // List MCP servers from project's .mcp.json
            let project = data.db.get_project(&channel_id_str);
            let disabled = data.db.get_disabled_mcps(&channel_id_str);

            let mcp_path = project
                .as_ref()
                .map(|p| std::path::PathBuf::from(&p.project_path).join(".mcp.json"))
                .unwrap_or_else(|| std::path::PathBuf::from(".mcp.json"));
            let servers = load_mcp_server_names(&mcp_path);

            if servers.is_empty() {
                return reply(ctx, cmd, "No MCP servers configured. Create `.mcp.json` in the project directory to add servers.").await;
            }

            let list: String = servers
                .iter()
                .map(|name| {
                    let status = if disabled.contains(name) {
                        "❌"
                    } else {
                        "✅"
                    };
                    format!("{status} `{name}`")
                })
                .collect::<Vec<_>>()
                .join("\n");

            let embed = CreateEmbed::new()
                .title("🔌 MCP Servers")
                .description(format!("{list}\n\nUse `/mcp <name>` to toggle."))
                .color(0x5865f2);

            reply_embed(ctx, cmd, embed).await
        }
    }
}

pub async fn autocomplete(ctx: &Context, auto: &CommandInteraction, data: &Arc<BotData>) {
    // Read from project dir, not cwd
    let channel_id_str = auto.channel_id.to_string();
    let mcp_path = data
        .db
        .get_project(&channel_id_str)
        .map(|p| std::path::PathBuf::from(&p.project_path).join(".mcp.json"))
        .unwrap_or_else(|| std::path::PathBuf::from(".mcp.json"));
    let servers = load_mcp_server_names(&mcp_path);

    let focused = auto
        .data
        .options
        .first()
        .and_then(|o| o.value.as_str())
        .unwrap_or("");

    let choices: Vec<AutocompleteChoice> = servers
        .into_iter()
        .filter(|s| s.to_lowercase().contains(&focused.to_lowercase()))
        .take(25)
        .map(|s| AutocompleteChoice::new(s.clone(), s))
        .collect();

    let _ = auto
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Autocomplete(
                CreateAutocompleteResponse::new().set_choices(choices),
            ),
        )
        .await;
}

fn load_mcp_server_names(path: &std::path::Path) -> Vec<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let servers = parsed
        .get("mcpServers")
        .or(Some(&parsed))
        .and_then(|v| v.as_object());

    match servers {
        Some(obj) => obj.keys().cloned().collect(),
        None => vec![],
    }
}
