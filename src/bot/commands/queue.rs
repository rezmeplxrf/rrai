use super::{BotData, reply, reply_embed_with_components};
use serenity::all::*;
use std::sync::Arc;

pub fn register() -> CreateCommand {
    CreateCommand::new("queue")
        .description("View or manage the message queue")
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "list",
            "Show queued messages",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "clear",
            "Clear all queued messages",
        ))
}

pub async fn run(
    ctx: &Context,
    cmd: &CommandInteraction,
    data: &Arc<BotData>,
) -> Result<(), String> {
    let action = cmd
        .data
        .options
        .first()
        .map(|o| o.name.as_str())
        .unwrap_or("list");

    let channel_id_str = cmd.channel_id.to_string();

    match action {
        "clear" => {
            let cleared = data.session_manager.clear_queue(&channel_id_str);
            reply(
                ctx,
                cmd,
                &format!("🗑️ Cleared {cleared} queued message(s)."),
            )
            .await
        }
        _ => {
            // list
            let queue = data.session_manager.get_queue_prompts(&channel_id_str);
            if queue.is_empty() {
                return reply(ctx, cmd, "📋 Queue is empty.").await;
            }

            let list: String = queue
                .iter()
                .enumerate()
                .map(|(idx, p)| {
                    let preview = if p.len() > 100 {
                        format!("{}…", crate::claude::output_formatter::truncate(p, 100))
                    } else {
                        p.clone()
                    };
                    format!("**{}.** {preview}", idx + 1)
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            let mut buttons: Vec<CreateButton> = queue
                .iter()
                .enumerate()
                .take(19)
                .map(|(idx, _)| {
                    CreateButton::new(format!("queue-remove:{}:{}", channel_id_str, idx))
                        .label(format!("❌ {}", idx + 1))
                        .style(ButtonStyle::Secondary)
                })
                .collect();
            buttons.push(
                CreateButton::new(format!("queue-clear:{}", channel_id_str))
                    .label("Clear All")
                    .style(ButtonStyle::Danger),
            );

            let mut rows: Vec<CreateActionRow> = Vec::new();
            for chunk in buttons.chunks(5) {
                rows.push(CreateActionRow::Buttons(chunk.to_vec()));
            }

            let embed = CreateEmbed::new()
                .title(format!("📋 Message Queue ({})", queue.len()))
                .description(list)
                .color(0x5865f2);

            reply_embed_with_components(ctx, cmd, embed, rows).await
        }
    }
}
