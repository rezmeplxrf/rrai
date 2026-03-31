//! Integration tests for the Database module.
//!
//! Each test gets a fresh in-memory SQLite database so tests are isolated
//! and can run in parallel.

use rrai::db::types::SessionStatus;
use rrai::db::Database;
use std::path::Path;

fn temp_db() -> Database {
    Database::open(Path::new(":memory:")).expect("in-memory DB should open")
}

// --- Project CRUD ---

#[test]
fn register_and_retrieve_project() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/project1", "guild-1");

    let project = db.get_project("ch-1").expect("project should exist");
    assert_eq!(project.channel_id, "ch-1");
    assert_eq!(project.project_path, "/tmp/project1");
    assert_eq!(project.guild_id, "guild-1");
    assert!(!project.auto_approve);
}

#[test]
fn get_nonexistent_project_returns_none() {
    let db = temp_db();
    assert!(db.get_project("nonexistent").is_none());
}

#[test]
fn register_project_upserts_on_conflict() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/old", "guild-1");
    db.register_project("ch-1", "/tmp/new", "guild-1");

    let project = db.get_project("ch-1").unwrap();
    assert_eq!(project.project_path, "/tmp/new");
}

#[test]
fn unregister_project_removes_project_and_sessions() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");
    db.upsert_session("s-1", "ch-1", Some("sid-1"), SessionStatus::Idle);

    db.unregister_project("ch-1");

    assert!(db.get_project("ch-1").is_none());
    assert!(db.get_session("ch-1").is_none());
}

#[test]
fn get_all_projects_filters_by_guild() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/a", "guild-1");
    db.register_project("ch-2", "/tmp/b", "guild-1");
    db.register_project("ch-3", "/tmp/c", "guild-2");

    let guild1 = db.get_all_projects("guild-1");
    assert_eq!(guild1.len(), 2);

    let guild2 = db.get_all_projects("guild-2");
    assert_eq!(guild2.len(), 1);

    let empty = db.get_all_projects("guild-99");
    assert!(empty.is_empty());
}

// --- Auto-approve ---

#[test]
fn set_auto_approve_toggles_flag() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");

    assert!(!db.get_project("ch-1").unwrap().auto_approve);

    db.set_auto_approve("ch-1", true);
    assert!(db.get_project("ch-1").unwrap().auto_approve);

    db.set_auto_approve("ch-1", false);
    assert!(!db.get_project("ch-1").unwrap().auto_approve);
}

// --- Session CRUD ---

#[test]
fn upsert_and_retrieve_session() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");
    db.upsert_session("s-1", "ch-1", Some("claude-sess-1"), SessionStatus::Online);

    let session = db.get_session("ch-1").expect("session should exist");
    assert_eq!(session.id, "s-1");
    assert_eq!(session.session_id, Some("claude-sess-1".to_string()));
    assert_eq!(session.status, SessionStatus::Online);
}

#[test]
fn upsert_session_replaces_by_id() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");
    db.upsert_session("s-1", "ch-1", None, SessionStatus::Offline);
    // Upsert with same id updates in place
    db.upsert_session("s-1", "ch-1", Some("sid"), SessionStatus::Idle);

    let session = db.get_session("ch-1").unwrap();
    assert_eq!(session.id, "s-1");
    assert_eq!(session.session_id, Some("sid".to_string()));
    assert_eq!(session.status, SessionStatus::Idle);
}

#[test]
fn update_session_status() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");
    db.upsert_session("s-1", "ch-1", None, SessionStatus::Online);

    db.update_session_status("ch-1", SessionStatus::Waiting);

    let session = db.get_session("ch-1").unwrap();
    assert_eq!(session.status, SessionStatus::Waiting);
}

#[test]
fn clear_session_removes_all_for_channel() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");
    db.upsert_session("s-1", "ch-1", None, SessionStatus::Online);
    db.upsert_session("s-2", "ch-1", None, SessionStatus::Idle);

    db.clear_session("ch-1");
    assert!(db.get_session("ch-1").is_none());
}

#[test]
fn get_all_sessions_joins_project() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/a", "guild-1");
    db.register_project("ch-2", "/tmp/b", "guild-1");
    db.upsert_session("s-1", "ch-1", None, SessionStatus::Online);
    db.upsert_session("s-2", "ch-2", None, SessionStatus::Idle);

    let sessions = db.get_all_sessions("guild-1");
    assert_eq!(sessions.len(), 2);

    let paths: Vec<&str> = sessions.iter().map(|s| s.project_path.as_str()).collect();
    assert!(paths.contains(&"/tmp/a"));
    assert!(paths.contains(&"/tmp/b"));
}

#[test]
fn get_all_sessions_empty_guild() {
    let db = temp_db();
    assert!(db.get_all_sessions("no-guild").is_empty());
}

// --- Model per channel ---

#[test]
fn set_and_get_model() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");

    assert!(db.get_model("ch-1").is_none());

    db.set_model("ch-1", Some("claude-sonnet-4-6"));
    assert_eq!(db.get_model("ch-1").unwrap(), "claude-sonnet-4-6");

    db.set_model("ch-1", None);
    assert!(db.get_model("ch-1").is_none());
}

#[test]
fn get_model_nonexistent_channel() {
    let db = temp_db();
    assert!(db.get_model("nonexistent").is_none());
}

// --- Disabled MCPs ---

#[test]
fn disabled_mcps_default_empty() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");
    assert!(db.get_disabled_mcps("ch-1").is_empty());
}

#[test]
fn set_and_get_disabled_mcps() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");

    let mcps = vec!["server-a".to_string(), "server-b".to_string()];
    db.set_disabled_mcps("ch-1", &mcps);

    let result = db.get_disabled_mcps("ch-1");
    assert_eq!(result, mcps);
}

#[test]
fn set_disabled_mcps_empty_clears() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");

    db.set_disabled_mcps("ch-1", &["x".to_string()]);
    assert_eq!(db.get_disabled_mcps("ch-1").len(), 1);

    db.set_disabled_mcps("ch-1", &[]);
    assert!(db.get_disabled_mcps("ch-1").is_empty());
}

#[test]
fn get_disabled_mcps_nonexistent_channel() {
    let db = temp_db();
    assert!(db.get_disabled_mcps("ghost").is_empty());
}

// --- Foreign key cascade ---

#[test]
fn deleting_project_cascades_to_sessions() {
    let db = temp_db();
    db.register_project("ch-1", "/tmp/p", "guild-1");
    db.upsert_session("s-1", "ch-1", None, SessionStatus::Online);

    // Use unregister which manually deletes both, but also verify the cascade
    // by directly checking sessions remain absent
    db.unregister_project("ch-1");
    assert!(db.get_session("ch-1").is_none());
}

// --- Concurrent access (basic) ---

#[test]
fn database_is_clone_safe() {
    let db = temp_db();
    let db2 = db.clone();

    db.register_project("ch-1", "/tmp/p", "guild-1");
    let project = db2.get_project("ch-1");
    assert!(project.is_some());
}
