use std::fs;
use std::path::Path;
use tracing::warn;

/// Remove the Claude session directory and uploads for a project.
pub fn cleanup_project_files(project_path: &str) {
    // Remove .claude-uploads directory
    let uploads_dir = Path::new(project_path).join(".claude-uploads");
    if uploads_dir.exists() {
        if let Err(e) = fs::remove_dir_all(&uploads_dir) {
            warn!("Failed to remove uploads dir {}: {e}", uploads_dir.display());
        }
    }

    // Remove session files from ~/.claude/projects/<encoded-path>/
    let home = match dirs_next() {
        Some(h) => h,
        None => return,
    };

    // Claude Code encodes the project path as the directory name
    let encoded = project_path.replace('/', "%2F");
    let session_dir = home.join(".claude").join("projects").join(&encoded);
    if session_dir.exists() {
        if let Err(e) = fs::remove_dir_all(&session_dir) {
            warn!("Failed to remove session dir {}: {e}", session_dir.display());
        }
    }
}

fn dirs_next() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}
