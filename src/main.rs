use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process;
use tracing::{error, info};

fn lock_file_path() -> PathBuf {
    rrai::config::get_config().lock_path()
}

fn acquire_lock() -> bool {
    let lock_path = lock_file_path();

    // Check for stale lock from a dead process
    if lock_path.exists() {
        if let Ok(content) = fs::read_to_string(&lock_path)
            && let Ok(pid) = content.trim().parse::<u32>()
        {
            // Check if process is still running (signal 0)
            let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
            if alive {
                return false; // process still running
            }
        }
        // Stale lock — remove it before attempting atomic create
        let _ = fs::remove_file(&lock_path);
    }

    // Atomic create: O_CREAT | O_EXCL ensures only one process wins
    let result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path);
    match result {
        Ok(mut f) => {
            let _ = f.write_all(process::id().to_string().as_bytes());
            true
        }
        Err(_) => false,
    }
}

fn release_lock() {
    let _ = fs::remove_file(lock_file_path());
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // #32: Set up panic hook to prevent silent task drops
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!("Task panicked: {info}");
        default_panic(info);
    }));

    // Load .env file
    dotenvy::dotenv().ok();

    info!("Starting RRAI — Rust Remote AI...");

    // Load and validate config first (needed for data_dir paths)
    if let Err(e) = rrai::config::load_config() {
        error!("Configuration error: {e}");
        process::exit(1);
    }
    let config = rrai::config::get_config();
    info!("Config loaded (data_dir: {})", config.data_dir);

    // Ensure data and sessions directories exist
    if let Err(e) = fs::create_dir_all(config.sessions_dir()) {
        error!("Failed to create sessions dir: {e}");
        process::exit(1);
    }

    if !acquire_lock() {
        error!("Another bot instance is already running. Exiting.");
        process::exit(1);
    }

    // Set up shutdown signal handler
    setup_signal_handlers();

    // Initialize database
    let db = match rrai::db::Database::open(&config.db_path()) {
        Ok(db) => db,
        Err(e) => {
            error!("Database error: {e}");
            release_lock();
            process::exit(1);
        }
    };
    info!("Database initialized");

    // Start Discord bot
    if let Err(e) = rrai::bot::client::start_bot(db).await {
        error!("Fatal error: {e}");
        graceful_shutdown().await;
        release_lock();
        process::exit(1);
    }
}

async fn graceful_shutdown() {
    if let Some(sm) = rrai::get_session_manager() {
        info!("Shutting down active sessions...");
        sm.shutdown().await;
        info!("All sessions stopped");
    }
}

/// Handle both SIGINT (Ctrl+C) and SIGTERM (systemd stop, kill).
fn setup_signal_handlers() {
    // SIGINT (Ctrl+C)
    tokio::spawn(async {
        let _ = tokio::signal::ctrl_c().await;
        info!("Received SIGINT, shutting down gracefully...");
        graceful_shutdown().await;
        release_lock();
        process::exit(0);
    });

    // SIGTERM
    #[cfg(unix)]
    tokio::spawn(async {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            sigterm.recv().await;
            info!("Received SIGTERM, shutting down gracefully...");
            graceful_shutdown().await;
            release_lock();
            process::exit(0);
        }
    });
}
