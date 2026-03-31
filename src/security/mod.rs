use crate::config::get_config;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Instant;

struct RateLimitEntry {
    count: u32,
    reset_at: Instant,
}

static RATE_LIMITS: LazyLock<Mutex<HashMap<u64, RateLimitEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn is_allowed_user(user_id: u64) -> bool {
    get_config().allowed_user_ids.contains(&user_id)
}

pub fn check_rate_limit(user_id: u64) -> bool {
    let config = get_config();
    let now = Instant::now();
    let window = std::time::Duration::from_secs(60);

    let mut limits = RATE_LIMITS.lock();
    let entry = limits.entry(user_id).or_insert(RateLimitEntry {
        count: 0,
        reset_at: now + window,
    });

    if now >= entry.reset_at {
        entry.count = 0;
        entry.reset_at = now + window;
    }

    entry.count += 1;
    entry.count <= config.rate_limit_per_minute
}

/// Periodically clean up expired rate limit entries.
pub fn cleanup_rate_limits() {
    let now = Instant::now();
    let mut limits = RATE_LIMITS.lock();
    limits.retain(|_, entry| now < entry.reset_at);
}

pub fn validate_project_path(project_path: &str) -> Option<String> {
    if project_path.contains("..") {
        return Some("Path must not contain '..'".to_string());
    }

    let config = get_config();
    let base_dir = match config.sessions_dir().canonicalize() {
        Ok(p) => p,
        Err(_) => return Some("Sessions directory does not exist".to_string()),
    };
    let resolved = Path::new(project_path).canonicalize().ok();

    match resolved {
        Some(p) if p.starts_with(&base_dir) => {
            if p.is_dir() {
                None // valid
            } else {
                Some(format!("Path is not a directory: {}", p.display()))
            }
        }
        Some(_) => Some(format!("Path must be within {}", base_dir.display())),
        None => Some(format!("Path does not exist: {project_path}")),
    }
}
