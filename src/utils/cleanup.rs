use crate::bot::handlers::interaction::find_session_dir;
use std::fs;
use std::path::Path;
use tracing::warn;

/// Remove the Claude session directory and uploads for a project.
pub fn cleanup_project_files(project_path: &str) {
    // Remove .claude-uploads directory
    let uploads_dir = Path::new(project_path).join(".claude-uploads");
    if uploads_dir.exists()
        && let Err(e) = fs::remove_dir_all(&uploads_dir)
    {
        warn!(
            "Failed to remove uploads dir {}: {e}",
            uploads_dir.display()
        );
    }

    // Remove session directory using the same lookup as find_session_dir
    if let Some(session_dir) = find_session_dir(project_path)
        && let Err(e) = fs::remove_dir_all(&session_dir)
    {
        warn!(
            "Failed to remove session dir {}: {e}",
            session_dir.display()
        );
    }
}
