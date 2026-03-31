mod agents;
mod auto_approve;
mod clear_sessions;
mod config_cmd;
mod context;
mod last;
mod mcp;
mod model;
mod models;
mod queue;
mod sessions;
mod skills;
mod start_new;
mod status;
mod stop;
mod usage;

use crate::bot::client::BotData;
use serenity::all::*;
use std::sync::Arc;

pub fn all_commands() -> Vec<CreateCommand> {
    vec![
        start_new::register(),
        status::register(),
        stop::register(),
        auto_approve::register(),
        sessions::register(),
        clear_sessions::register(),
        last::register(),
        queue::register(),
        usage::register(),
        model::register(),
        models::register(),
        context::register(),
        mcp::register(),
        config_cmd::register(),
        skills::register(),
        agents::register(),
    ]
}

pub async fn handle_command(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    match cmd.data.name.as_str() {
        "start-new" => start_new::run(ctx, cmd, data).await,
        "status" => status::run(ctx, cmd, data).await,
        "stop" => stop::run(ctx, cmd, data).await,
        "auto-approve" => auto_approve::run(ctx, cmd, data).await,
        "sessions" => sessions::run(ctx, cmd, data).await,
        "clear" => clear_sessions::run(ctx, cmd, data).await,
        "last" => last::run(ctx, cmd, data).await,
        "queue" => queue::run(ctx, cmd, data).await,
        "usage" => usage::run(ctx, cmd, data).await,
        "model" => model::run(ctx, cmd, data).await,
        "models" => models::run(ctx, cmd, data).await,
        "context" => context::run(ctx, cmd, data).await,
        "mcp" => mcp::run(ctx, cmd, data).await,
        "config" => config_cmd::run(ctx, cmd, data).await,
        "skills" => skills::run(ctx, cmd, data).await,
        "agents" => agents::run(ctx, cmd, data).await,
        _ => Ok(()),
    }
}

pub async fn handle_autocomplete(ctx: &Context, auto: &CommandInteraction, data: &Arc<BotData>) {
    if auto.data.name.as_str() == "mcp" {
        mcp::autocomplete(ctx, auto, data).await;
    }
}

/// Helper to edit the deferred reply.
pub(crate) async fn reply(
    ctx: &Context,
    cmd: &CommandInteraction,
    content: &str,
) -> Result<(), String> {
    cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(content))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn reply_embed(
    ctx: &Context,
    cmd: &CommandInteraction,
    embed: CreateEmbed,
) -> Result<(), String> {
    cmd.edit_response(&ctx.http, EditInteractionResponse::new().embed(embed))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn reply_embed_with_components(
    ctx: &Context,
    cmd: &CommandInteraction,
    embed: CreateEmbed,
    components: Vec<CreateActionRow>,
) -> Result<(), String> {
    cmd.edit_response(
        &ctx.http,
        EditInteractionResponse::new()
            .embed(embed)
            .components(components),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
