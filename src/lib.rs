pub mod bot;
pub mod claude;
pub mod config;
pub mod db;
pub mod discord;
pub mod security;
pub mod utils;

use std::sync::{Arc, OnceLock};

/// Global handle for graceful shutdown from signal handlers.
static GLOBAL_SESSION_MANAGER: OnceLock<Arc<claude::session_manager::SessionManager>> =
    OnceLock::new();

/// Register the session manager globally so signal handlers can access it.
pub fn register_session_manager(sm: Arc<claude::session_manager::SessionManager>) {
    let _ = GLOBAL_SESSION_MANAGER.set(sm);
}

/// Get the global session manager (if registered).
pub fn get_session_manager() -> Option<&'static Arc<claude::session_manager::SessionManager>> {
    GLOBAL_SESSION_MANAGER.get()
}
