use super::{reply, BotData};
use crate::db::types::SessionStatus;
use crate::utils::channel_name::to_channel_name;
use serenity::all::*;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

pub fn register() -> CreateCommand {
    CreateCommand::new("start-new")
        .description("Create a new channel and start a Claude session")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "message",
                "First message to send to Claude",
            )
            .required(true),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "channel-name",
                "Custom channel name (auto-generated if not provided)",
            )
            .required(false),
        )
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let message = cmd
        .data
        .options
        .iter()
        .find(|o| o.name == "message")
        .and_then(|o| o.value.as_str())
        .unwrap_or("")
        .to_string();

    if message.is_empty() {
        return reply(ctx, cmd, "Please provide a message.").await;
    }

    let custom_name = cmd
        .data
        .options
        .iter()
        .find(|o| o.name == "channel-name")
        .and_then(|o| o.value.as_str())
        .map(|s| s.to_string());

    let guild_id = match cmd.guild_id {
        Some(g) => g,
        None => return reply(ctx, cmd, "This command can only be used in a server.").await,
    };

    // Generate channel name from message or custom name
    let channel_name = match custom_name {
        Some(name) => to_channel_name(&name),
        None => {
            let slug = to_channel_name(&message);
            if slug.len() > 30 {
                slug[..30].trim_end_matches('-').to_string()
            } else {
                slug
            }
        }
    };

    // Create a new text channel
    let channel = guild_id
        .create_channel(
            &ctx.http,
            CreateChannel::new(&channel_name).kind(ChannelType::Text),
        )
        .await
        .map_err(|e| format!("Failed to create channel: {e}"))?;

    // Register the project
    let config = crate::config::get_config();
    let project_path = Path::new(&config.base_project_dir).join(&channel_name);
    std::fs::create_dir_all(&project_path).ok();
    data.db.register_project(
        &channel.id.to_string(),
        &project_path.to_string_lossy(),
        &guild_id.to_string(),
    );
    data.db.upsert_session(
        &Uuid::new_v4().to_string(),
        &channel.id.to_string(),
        None,
        SessionStatus::Idle,
    );

    reply(
        ctx,
        cmd,
        &format!("✨ Created <#{}>. Sending your message...", channel.id),
    )
    .await?;

    // Send the first message to the new channel's Claude session
    let sm = data.session_manager.clone();
    let ch_id = channel.id;
    tokio::spawn(async move {
        if let Err(e) = sm.send_message(ch_id, guild_id, &message).await {
            warn!("[start-new] sendMessage error: {e}");
        }
    });

    Ok(())
}

use tracing::warn;
