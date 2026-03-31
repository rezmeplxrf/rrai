use super::{BotData, reply};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("model")
        .description("Set or view the Claude model for this channel")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "name",
                "Model name (leave empty to view current)",
            )
            .required(false)
            .add_string_choice("Opus (latest)", "opus")
            .add_string_choice("Sonnet (latest)", "sonnet")
            .add_string_choice("Haiku (latest)", "haiku")
            .add_string_choice("Default", "default"),
        )
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let channel_id_str = cmd.channel_id.to_string();

    let model_name = cmd
        .data
        .options
        .first()
        .and_then(|o| o.value.as_str())
        .map(|s| s.to_string());

    match model_name {
        Some(name) if name == "reset" || name == "default" => {
            if let Err(e) = data.db.set_model(&channel_id_str, None) {
                tracing::warn!("Failed to reset model: {e}");
            }
            // Restart session with new model
            data.session_manager.stop_session(&channel_id_str).await;
            reply(ctx, cmd, "🔄 Model reset to default. Session restarted.").await
        }
        Some(name) => {
            if let Err(e) = data.db.set_model(&channel_id_str, Some(&name)) {
                tracing::warn!("Failed to set model: {e}");
            }
            // Restart session with new model
            data.session_manager.stop_session(&channel_id_str).await;
            reply(
                ctx,
                cmd,
                &format!("🤖 Model set to `{name}`. Session restarted."),
            )
            .await
        }
        None => {
            let current = data.db.get_model(&channel_id_str);
            match current {
                Some(model) => reply(ctx, cmd, &format!("Current model: `{model}`")).await,
                None => reply(ctx, cmd, "Using default model.").await,
            }
        }
    }
}
