use serenity::all::{
    ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateSelectMenu,
    CreateSelectMenuKind, CreateSelectMenuOption,
};

const MAX_DISCORD_LENGTH: usize = 1900;

pub fn format_stream_chunk(text: &str) -> String {
    if text.len() <= MAX_DISCORD_LENGTH {
        text.to_string()
    } else {
        format!("{}\n... (truncated)", &text[..MAX_DISCORD_LENGTH])
    }
}

pub fn split_message(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut remaining = text.to_string();

    while !remaining.is_empty() {
        if remaining.len() <= MAX_DISCORD_LENGTH {
            chunks.push(remaining);
            break;
        }

        // Try to split at a newline
        let search_range = &remaining[..MAX_DISCORD_LENGTH];
        let split_at = match search_range.rfind('\n') {
            Some(pos) if pos >= MAX_DISCORD_LENGTH / 2 => pos,
            _ => MAX_DISCORD_LENGTH,
        };

        let mut chunk = remaining[..split_at].to_string();
        remaining = remaining[split_at..].to_string();

        // Check if we're splitting inside an unclosed code block
        // Only match code fences at the start of a line (like the TS regex /^```/gm)
        let mut inside_block = false;
        let mut block_lang = String::new();
        for line in chunk.lines() {
            if line.starts_with("```") {
                let trimmed = line;
                if inside_block {
                    inside_block = false;
                    block_lang.clear();
                } else {
                    inside_block = true;
                    block_lang = trimmed[3..].trim().to_string();
                }
            }
        }

        if inside_block {
            chunk.push_str("\n```");
            remaining = format!("```{block_lang}\n{remaining}");
        }

        chunks.push(chunk);
    }

    chunks
}

pub fn create_stop_button(channel_id: &str) -> CreateActionRow {
    CreateActionRow::Buttons(vec![CreateButton::new(format!("stop:{channel_id}"))
        .label("Stop")
        .style(ButtonStyle::Danger)
        .emoji('⏹')])
}

pub fn create_completed_button() -> CreateActionRow {
    CreateActionRow::Buttons(vec![CreateButton::new("completed")
        .label("Completed")
        .style(ButtonStyle::Secondary)
        .emoji('✅')
        .disabled(true)])
}

pub fn create_tool_approval_embed(
    tool_name: &str,
    input: &serde_json::Value,
    request_id: &str,
) -> (CreateEmbed, CreateActionRow) {
    let escaped_name = tool_name.replace('_', "\\_");
    let mut embed = CreateEmbed::new()
        .title(format!("🔧 Tool Use: {escaped_name}"))
        .color(0xffa500)
        .timestamp(serenity::model::Timestamp::now());

    match tool_name {
        "Edit" | "Write" => {
            let file_path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            embed = embed.field("File", format!("`{file_path}`"), false);

            if let (Some(old), Some(new)) = (
                input.get("old_string").and_then(|v| v.as_str()),
                input.get("new_string").and_then(|v| v.as_str()),
            ) {
                let old_preview = truncate(old, 500);
                let new_preview = truncate(new, 500);
                embed = embed.field(
                    "Changes",
                    format!("```diff\n- {old_preview}\n+ {new_preview}\n```"),
                    false,
                );
            } else if let Some(content) = input.get("content").and_then(|v| v.as_str()) {
                let preview = truncate(content, 500);
                embed = embed.field("Content Preview", format!("```\n{preview}\n```"), false);
            }
        }
        "Bash" => {
            let command = input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            embed = embed.field("Command", format!("```bash\n{command}\n```"), false);
            if let Some(desc) = input.get("description").and_then(|v| v.as_str()) {
                if !desc.is_empty() {
                    embed = embed.field("Description", desc, false);
                }
            }
        }
        _ => {
            let summary = serde_json::to_string_pretty(input).unwrap_or_default();
            if !summary.is_empty() && summary != "{}" {
                let preview = truncate(&summary, 800);
                embed = embed.field("Input", format!("```json\n{preview}\n```"), false);
            }
        }
    }

    let row = CreateActionRow::Buttons(vec![
        CreateButton::new(format!("approve:{request_id}"))
            .label("Approve")
            .style(ButtonStyle::Success)
            .emoji('✅'),
        CreateButton::new(format!("deny:{request_id}"))
            .label("Deny")
            .style(ButtonStyle::Danger)
            .emoji('❌'),
        CreateButton::new(format!("approve-all:{request_id}"))
            .label("Auto-approve All")
            .style(ButtonStyle::Secondary)
            .emoji('⚡'),
    ]);

    (embed, row)
}

pub struct AskQuestionData {
    pub question: String,
    pub header: String,
    pub options: Vec<AskOption>,
    pub multi_select: bool,
}

pub struct AskOption {
    pub label: String,
    pub description: String,
}

pub fn create_ask_user_question_embed(
    question_data: &AskQuestionData,
    request_id: &str,
    question_index: usize,
    total_questions: usize,
) -> (CreateEmbed, Vec<CreateActionRow>) {
    let title = if total_questions > 1 {
        format!(
            "❓ {} ({}/{})",
            question_data.header,
            question_index + 1,
            total_questions
        )
    } else {
        format!("❓ {}", question_data.header)
    };

    let mut embed = CreateEmbed::new()
        .title(title)
        .description(&question_data.question)
        .color(0x7c3aed)
        .timestamp(serenity::model::Timestamp::now());

    for opt in &question_data.options {
        let desc = if opt.description.is_empty() {
            "\u{200b}".to_string()
        } else {
            opt.description.clone()
        };
        embed = embed.field(&opt.label, desc, false);
    }

    let mut components = Vec::new();

    if question_data.multi_select {
        let options: Vec<CreateSelectMenuOption> = question_data
            .options
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                let mut menu_opt =
                    CreateSelectMenuOption::new(truncate(&opt.label, 100), i.to_string());
                if !opt.description.is_empty() {
                    menu_opt = menu_opt.description(truncate(&opt.description, 100));
                }
                menu_opt
            })
            .collect();

        let max_vals = question_data.options.len().min(25) as u8;
        let select_menu = CreateSelectMenu::new(
            format!("ask-select:{request_id}"),
            CreateSelectMenuKind::String { options },
        )
        .placeholder("Select options...")
        .min_values(1)
        .max_values(max_vals);

        components.push(CreateActionRow::SelectMenu(select_menu));
        components.push(CreateActionRow::Buttons(vec![
            CreateButton::new(format!("ask-other:{request_id}"))
                .label("Custom input")
                .style(ButtonStyle::Secondary)
                .emoji('✏'),
        ]));
    } else {
        let mut buttons: Vec<CreateButton> = question_data
            .options
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                CreateButton::new(format!("ask-opt:{request_id}:{i}"))
                    .label(truncate(&opt.label, 80))
                    .style(if i == 0 {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    })
            })
            .collect();

        buttons.push(
            CreateButton::new(format!("ask-other:{request_id}"))
                .label("Custom input")
                .style(ButtonStyle::Secondary)
                .emoji('✏'),
        );

        // Discord max 5 buttons per row
        for chunk in buttons.chunks(5) {
            components.push(CreateActionRow::Buttons(chunk.to_vec()));
        }
    }

    (embed, components)
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub fn create_result_embed(
    result: &str,
    input_tokens: u64,
    output_tokens: u64,
    duration_ms: u64,
    cost_usd: Option<f64>,
) -> (CreateEmbed, Option<Vec<u8>>) {
    let duration = format!("{:.1}s", duration_ms as f64 / 1000.0);
    let total_tokens = input_tokens + output_tokens;
    let mut footer = format!(
        "Tokens : {} (↑{} ↓{})  |  Duration : {}",
        format_tokens(total_tokens),
        format_tokens(input_tokens),
        format_tokens(output_tokens),
        duration
    );
    if let Some(cost) = cost_usd {
        footer.push_str(&format!("  |  Cost : ${cost:.4}"));
    }

    let needs_file = result.len() > 4000;
    let description = if needs_file {
        format!(
            "{}\n\n... Full result attached as file",
            &result[..3900.min(result.len())]
        )
    } else {
        result.to_string()
    };

    let embed = CreateEmbed::new()
        .title("✅ Task Complete")
        .description(description)
        .color(0x00ff00)
        .footer(serenity::all::CreateEmbedFooter::new(footer))
        .timestamp(serenity::model::Timestamp::now());

    let file = if needs_file {
        Some(result.as_bytes().to_vec())
    } else {
        None
    };

    (embed, file)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Find a valid char boundary
        let mut end = max;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &s[..end]
    }
}
