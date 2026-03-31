use once_cell::sync::OnceCell;
use std::env;

static CONFIG: OnceCell<Config> = OnceCell::new();

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_bot_token: String,
    pub discord_guild_id: u64,
    pub allowed_user_ids: Vec<u64>,
    pub base_project_dir: String,
    pub rate_limit_per_minute: u32,
    pub show_cost: bool,
}

impl Config {
    fn from_env() -> Result<Self, String> {
        let token = required_env("DISCORD_BOT_TOKEN")?;
        let guild_id = required_env("DISCORD_GUILD_ID")?
            .parse::<u64>()
            .map_err(|_| "DISCORD_GUILD_ID must be a valid integer".to_string())?;

        let user_ids_str = required_env("ALLOWED_USER_IDS")?;
        let allowed_user_ids: Vec<u64> = user_ids_str
            .split(',')
            .map(|s| {
                s.trim()
                    .parse::<u64>()
                    .map_err(|_| format!("Invalid user ID: {}", s.trim()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if allowed_user_ids.is_empty() {
            return Err("ALLOWED_USER_IDS must contain at least one user ID".to_string());
        }

        let base_project_dir = required_env("BASE_PROJECT_DIR")?;

        let rate_limit_per_minute = env::var("RATE_LIMIT_PER_MINUTE")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u32>()
            .map_err(|_| "RATE_LIMIT_PER_MINUTE must be a positive integer".to_string())?;

        let show_cost = env::var("SHOW_COST")
            .unwrap_or_else(|_| "true".to_string())
            .to_lowercase()
            == "true";

        Ok(Config {
            discord_bot_token: token,
            discord_guild_id: guild_id,
            allowed_user_ids,
            base_project_dir,
            rate_limit_per_minute,
            show_cost,
        })
    }
}

fn required_env(key: &str) -> Result<String, String> {
    env::var(key)
        .map_err(|_| format!("{key} is required"))
        .and_then(|v| {
            if v.is_empty() {
                Err(format!("{key} must not be empty"))
            } else {
                Ok(v)
            }
        })
}

pub fn load_config() -> Result<&'static Config, String> {
    CONFIG.get_or_try_init(Config::from_env)
}

pub fn get_config() -> &'static Config {
    CONFIG.get().expect("Config not initialized — call load_config() first")
}
