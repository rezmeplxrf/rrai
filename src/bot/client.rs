use crate::bot::commands;
use crate::bot::handlers::{interaction, message};
use crate::claude::session_manager::SessionManager;
use crate::config::get_config;
use crate::db::Database;
use crate::security::is_allowed_user;
use crate::utils::cleanup::cleanup_project_files;
use serenity::all::*;
use std::sync::Arc;
use tracing::{error, info};

pub struct BotData {
    pub db: Database,
    pub session_manager: Arc<SessionManager>,
}

pub struct Handler {
    pub data: Arc<BotData>,
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Bot logged in as {}", ready.user.name);

        let config = get_config();
        let guild_id = GuildId::new(config.discord_guild_id);

        // Register slash commands
        match guild_id
            .set_commands(
                &ctx.http,
                commands::all_commands(),
            )
            .await
        {
            Ok(cmds) => info!("Registered {} slash commands", cmds.len()),
            Err(e) => error!("Failed to register slash commands: {e}"),
        }

        // Clean up orphaned projects
        let data = self.data.clone();
        let http = ctx.http.clone();
        tokio::spawn(async move {
            loop {
                cleanup_orphaned_projects(&data.db, &http, guild_id).await;
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });

        // Periodic rate limit cleanup
        tokio::spawn(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                crate::security::cleanup_rate_limits();
            }
        });
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(cmd) => {
                if !is_allowed_user(cmd.user.id.get()) {
                    let _ = cmd
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("You are not authorized to use this bot.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                }

                // Defer reply to avoid 3-second timeout
                let _ = cmd
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Defer(
                            CreateInteractionResponseMessage::new(),
                        ),
                    )
                    .await;

                if let Err(e) = commands::handle_command(&ctx, &cmd, &self.data).await {
                    let _ = cmd
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .content(format!("An error occurred: {e}")),
                        )
                        .await;
                }
            }
            Interaction::Component(component) => {
                match &component.data.kind {
                    serenity::all::ComponentInteractionDataKind::Button => {
                        interaction::handle_button_interaction(
                            &ctx,
                            &component,
                            &self.data.db,
                            &self.data.session_manager,
                        )
                        .await;
                    }
                    serenity::all::ComponentInteractionDataKind::StringSelect { .. } => {
                        interaction::handle_select_menu_interaction(
                            &ctx,
                            &component,
                            &self.data.db,
                            &self.data.session_manager,
                        )
                        .await;
                    }
                    _ => {}
                }
            }
            Interaction::Autocomplete(auto) => {
                commands::handle_autocomplete(&ctx, &auto, &self.data).await;
            }
            _ => {}
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        message::handle_message(&ctx, &msg, &self.data.db, &self.data.session_manager).await;
    }
}

async fn cleanup_orphaned_projects(db: &Database, http: &Http, guild_id: GuildId) {
    let projects = db.get_all_projects(&guild_id.to_string());
    let mut cleaned = 0;

    for project in &projects {
        let channel_id = match project.channel_id.parse::<u64>() {
            Ok(id) => ChannelId::new(id),
            Err(_) => continue,
        };

        // Check if channel still exists
        match http.get_channel(channel_id).await {
            Ok(_) => {} // Channel exists
            Err(_) => {
                cleanup_project_files(&project.project_path);
                db.unregister_project(&project.channel_id);
                cleaned += 1;
            }
        }
    }

    if cleaned > 0 {
        info!("[cleanup] Removed {cleaned} orphaned project(s) for deleted channels");
    }
}

pub async fn start_bot(db: Database) -> Result<(), Box<dyn std::error::Error>> {
    let config = get_config();

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let http = Arc::new(Http::new(&config.discord_bot_token));
    let session_manager = SessionManager::new(db.clone(), http.clone());

    let data = Arc::new(BotData {
        db,
        session_manager,
    });

    let handler = Handler { data };

    let mut client = Client::builder(&config.discord_bot_token, intents)
        .event_handler(handler)
        .await?;

    // Login with retry
    let delays = [5, 10, 15, 30, 30, 30];
    let mut attempt = 0;

    loop {
        match client.start().await {
            Ok(_) => return Ok(()),
            Err(e) => {
                attempt += 1;
                let delay = delays[attempt.min(delays.len()) - 1];
                error!("Discord login attempt {attempt} failed: {e}");
                info!("Retrying in {delay}s...");
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }
        }
    }
}
