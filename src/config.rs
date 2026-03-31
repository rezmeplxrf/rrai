use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;

static CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_bot_token: String,
    pub discord_guild_id: u64,
    pub allowed_user_ids: Vec<u64>,
    /// Root data directory (~/.rrai by default)
    pub data_dir: String,
    pub rate_limit_per_minute: u32,
    /// Optional channel ID for broadcasting status changes
    pub status_channel_id: Option<u64>,
    /// Discord message edit throttle interval in milliseconds (default: 1500)
    pub edit_interval_ms: u128,
    /// Timeout for tool approval in seconds (default: 300)
    pub approval_timeout_secs: u64,
    /// Maximum queued messages per channel (default: 5)
    pub max_queue_size: usize,
    /// Timeout for SDK control commands in seconds (default: 15)
    pub sdk_call_timeout_secs: u64,
}

impl Config {
    /// Returns the sessions directory: {data_dir}/sessions
    pub fn sessions_dir(&self) -> PathBuf {
        PathBuf::from(&self.data_dir).join("sessions")
    }

    /// Returns the database path: {data_dir}/data.db
    pub fn db_path(&self) -> PathBuf {
        PathBuf::from(&self.data_dir).join("data.db")
    }

    /// Returns the lock file path: {data_dir}/.bot.lock
    pub fn lock_path(&self) -> PathBuf {
        PathBuf::from(&self.data_dir).join(".bot.lock")
    }

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

        let data_dir = env::var("RRAI_DATA_DIR").unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".rrai")
                .to_string_lossy()
                .to_string()
        });

        let rate_limit_per_minute = env::var("RATE_LIMIT_PER_MINUTE")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u32>()
            .map_err(|_| "RATE_LIMIT_PER_MINUTE must be a positive integer".to_string())?;

        if rate_limit_per_minute == 0 {
            return Err("RATE_LIMIT_PER_MINUTE must be greater than 0".to_string());
        }

        let status_channel_id = env::var("STATUS_CHANNEL_ID")
            .ok()
            .and_then(|s| s.parse::<u64>().ok());

        let edit_interval_ms = env::var("EDIT_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse::<u128>().ok())
            .unwrap_or(1500);

        let approval_timeout_secs = env::var("APPROVAL_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(300);

        let max_queue_size = env::var("MAX_QUEUE_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(5);

        let sdk_call_timeout_secs = env::var("SDK_CALL_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(15);

        Ok(Config {
            discord_bot_token: token,
            discord_guild_id: guild_id,
            allowed_user_ids,
            data_dir,
            rate_limit_per_minute,
            status_channel_id,
            edit_interval_ms,
            approval_timeout_secs,
            max_queue_size,
            sdk_call_timeout_secs,
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
    if let Some(c) = CONFIG.get() {
        return Ok(c);
    }
    let cfg = Config::from_env()?;
    Ok(CONFIG.get_or_init(|| cfg))
}

pub fn get_config() -> &'static Config {
    CONFIG
        .get()
        .expect("Config not initialized — call load_config() first")
}
