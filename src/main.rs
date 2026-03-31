mod bot;
mod claude;
mod config;
mod db;
mod security;
mod utils;

use std::fs;
use std::path::PathBuf;
use std::process;
use tracing::{error, info};

fn lock_file_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".bot.lock")
}

fn acquire_lock() -> bool {
    let lock_path = lock_file_path();
    if lock_path.exists() {
        if let Ok(content) = fs::read_to_string(&lock_path) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                // Check if process is still running (signal 0)
                unsafe {
                    if libc::kill(pid as i32, 0) == 0 {
                        return false; // process still running
                    }
                }
            }
        }
        // Stale lock file
    }
    fs::write(&lock_path, process::id().to_string()).is_ok()
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

    if !acquire_lock() {
        error!("Another bot instance is already running. Exiting.");
        process::exit(1);
    }

    // #26: Release lock on SIGINT and SIGTERM
    setup_signal_handlers();

    info!("Starting RRAI — Rust Remote AI...");

    // Load and validate config
    if let Err(e) = config::load_config() {
        error!("Configuration error: {e}");
        release_lock();
        process::exit(1);
    }
    info!("Config loaded");

    // Initialize database
    let db_path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("data.db");
    let db = match db::Database::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            error!("Database error: {e}");
            release_lock();
            process::exit(1);
        }
    };
    info!("Database initialized");

    // Start Discord bot
    if let Err(e) = bot::client::start_bot(db).await {
        error!("Fatal error: {e}");
        release_lock();
        process::exit(1);
    }
}

/// Handle both SIGINT (Ctrl+C) and SIGTERM (systemd stop, kill).
fn setup_signal_handlers() {
    // SIGINT (Ctrl+C)
    tokio::spawn(async {
        let _ = tokio::signal::ctrl_c().await;
        release_lock();
        process::exit(0);
    });

    // SIGTERM
    #[cfg(unix)]
    tokio::spawn(async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            sigterm.recv().await;
            release_lock();
            process::exit(0);
        }
    });
}
