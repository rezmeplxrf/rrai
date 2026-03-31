//! Integration tests for SessionManager using MockDiscordClient.
//!
//! These tests exercise the queue management, approval resolution,
//! custom input, and state-tracking logic without needing a real
//! Claude subprocess or Discord connection.

use rrai::claude::session_manager::SessionManager;
use rrai::db::Database;
use rrai::discord::{DiscordClient, MockDiscordClient};
use serenity::all::{ChannelId, GuildId};
use std::path::Path;
use std::sync::Arc;

fn setup() -> (Arc<SessionManager>, Arc<MockDiscordClient>, Database) {
    let db = Database::open(Path::new(":memory:")).expect("in-memory DB");
    let mock = Arc::new(MockDiscordClient::new());
    let sm = SessionManager::new_with_settings(
        db.clone(),
        mock.clone(),
        1500, // edit_interval_ms
        300,  // approval_timeout_secs
        5,    // max_queue_size
        15,   // sdk_call_timeout_secs
    );
    (sm, mock, db)
}

fn channel(id: u64) -> ChannelId {
    ChannelId::new(id)
}

fn guild(id: u64) -> GuildId {
    GuildId::new(id)
}

// --- Queue management ---

#[test]
fn queue_starts_empty() {
    let (sm, _, _) = setup();
    assert_eq!(sm.get_queue_size("ch-1"), 0);
    assert!(sm.get_queue_prompts("ch-1").is_empty());
    assert!(!sm.is_queue_full("ch-1"));
}

#[test]
fn set_pending_and_confirm_queue() {
    let (sm, _, _) = setup();
    sm.set_pending_queue("ch-1", channel(1), guild(1), "hello");
    assert!(sm.has_pending_queue("ch-1"));

    let confirmed = sm.confirm_queue("ch-1");
    assert!(confirmed);
    assert_eq!(sm.get_queue_size("ch-1"), 1);
    assert_eq!(sm.get_queue_prompts("ch-1"), vec!["hello"]);
    assert!(!sm.has_pending_queue("ch-1"));
}

#[test]
fn cancel_queue_removes_pending() {
    let (sm, _, _) = setup();
    sm.set_pending_queue("ch-1", channel(1), guild(1), "hello");
    sm.cancel_queue("ch-1");
    assert!(!sm.has_pending_queue("ch-1"));
    assert_eq!(sm.get_queue_size("ch-1"), 0);
}

#[test]
fn confirm_without_pending_returns_false() {
    let (sm, _, _) = setup();
    assert!(!sm.confirm_queue("ch-1"));
}

#[test]
fn queue_respects_max_size() {
    let (sm, _, _) = setup();
    for i in 0..5 {
        sm.set_pending_queue("ch-1", channel(1), guild(1), &format!("msg-{i}"));
        sm.confirm_queue("ch-1");
    }
    assert!(sm.is_queue_full("ch-1"));
    assert_eq!(sm.get_queue_size("ch-1"), 5);
}

#[test]
fn queue_multiple_prompts_ordering() {
    let (sm, _, _) = setup();
    for msg in &["first", "second", "third"] {
        sm.set_pending_queue("ch-1", channel(1), guild(1), msg);
        sm.confirm_queue("ch-1");
    }
    assert_eq!(
        sm.get_queue_prompts("ch-1"),
        vec!["first", "second", "third"]
    );
}

#[test]
fn clear_queue_returns_count() {
    let (sm, _, _) = setup();
    for i in 0..3 {
        sm.set_pending_queue("ch-1", channel(1), guild(1), &format!("msg-{i}"));
        sm.confirm_queue("ch-1");
    }
    let cleared = sm.clear_queue("ch-1");
    assert_eq!(cleared, 3);
    assert_eq!(sm.get_queue_size("ch-1"), 0);
}

#[test]
fn clear_empty_queue_returns_zero() {
    let (sm, _, _) = setup();
    assert_eq!(sm.clear_queue("ch-1"), 0);
}

#[test]
fn remove_from_queue_by_index() {
    let (sm, _, _) = setup();
    for msg in &["a", "b", "c"] {
        sm.set_pending_queue("ch-1", channel(1), guild(1), msg);
        sm.confirm_queue("ch-1");
    }

    let removed = sm.remove_from_queue("ch-1", 1);
    assert_eq!(removed, Some("b".to_string()));
    assert_eq!(sm.get_queue_prompts("ch-1"), vec!["a", "c"]);
}

#[test]
fn remove_from_queue_invalid_index() {
    let (sm, _, _) = setup();
    sm.set_pending_queue("ch-1", channel(1), guild(1), "only");
    sm.confirm_queue("ch-1");

    assert!(sm.remove_from_queue("ch-1", 5).is_none());
    assert_eq!(sm.get_queue_size("ch-1"), 1);
}

#[test]
fn remove_last_item_cleans_up_queue() {
    let (sm, _, _) = setup();
    sm.set_pending_queue("ch-1", channel(1), guild(1), "only");
    sm.confirm_queue("ch-1");

    sm.remove_from_queue("ch-1", 0);
    assert_eq!(sm.get_queue_size("ch-1"), 0);
}

// --- Queues are per-channel ---

#[test]
fn queues_isolated_per_channel() {
    let (sm, _, _) = setup();
    sm.set_pending_queue("ch-1", channel(1), guild(1), "msg-a");
    sm.confirm_queue("ch-1");

    sm.set_pending_queue("ch-2", channel(2), guild(1), "msg-b");
    sm.confirm_queue("ch-2");

    assert_eq!(sm.get_queue_size("ch-1"), 1);
    assert_eq!(sm.get_queue_size("ch-2"), 1);
    assert_eq!(sm.get_queue_prompts("ch-1"), vec!["msg-a"]);
    assert_eq!(sm.get_queue_prompts("ch-2"), vec!["msg-b"]);
}

// --- Approval resolution ---

#[test]
fn resolve_approval_nonexistent_returns_false() {
    let (sm, _, _) = setup();
    assert!(!sm.resolve_approval("nonexistent", "approve"));
}

// --- Custom input ---

#[test]
fn custom_input_lifecycle() {
    let (sm, _, _) = setup();

    assert!(!sm.has_pending_custom_input("ch-1"));

    sm.enable_custom_input("req-1", "ch-1");
    assert!(sm.has_pending_custom_input("ch-1"));

    // Resolve without a matching question → returns false (no question pending)
    let resolved = sm.resolve_custom_input("ch-1", "my answer");
    assert!(!resolved);

    // After resolve attempt, custom input is consumed
    assert!(!sm.has_pending_custom_input("ch-1"));
}

#[test]
fn custom_input_nonexistent_channel() {
    let (sm, _, _) = setup();
    assert!(!sm.resolve_custom_input("ghost", "text"));
}

// --- Session state (no active sessions) ---

#[tokio::test]
async fn is_busy_returns_false_no_session() {
    let (sm, _, _) = setup();
    assert!(!sm.is_busy("ch-1").await);
}

#[tokio::test]
async fn is_active_returns_false_no_session() {
    let (sm, _, _) = setup();
    assert!(!sm.is_active("ch-1").await);
}

#[tokio::test]
async fn stop_nonexistent_session_returns_false() {
    let (sm, _, _) = setup();
    assert!(!sm.stop_session("ch-1").await);
}

// --- Mock Discord client verification ---

#[tokio::test]
async fn mock_tracks_calls() {
    let mock = Arc::new(MockDiscordClient::new());
    mock.set_channel_name("test-channel");

    let name = mock.get_channel_name(channel(42)).await;
    assert_eq!(name, Some("test-channel".to_string()));
    assert_eq!(mock.calls().len(), 1);
}

#[tokio::test]
async fn mock_send_returns_incrementing_ids() {
    use serenity::all::CreateMessage;

    let mock = Arc::new(MockDiscordClient::new());
    let id1 = mock
        .send_message(channel(1), CreateMessage::new().content("a"))
        .await
        .unwrap();
    let id2 = mock
        .send_message(channel(1), CreateMessage::new().content("b"))
        .await
        .unwrap();
    assert_eq!(id1.get(), 1);
    assert_eq!(id2.get(), 2);
    assert_eq!(mock.count_sends(), 2);
}

#[tokio::test]
async fn mock_send_error() {
    use serenity::all::CreateMessage;

    let mock = Arc::new(MockDiscordClient::new());
    mock.set_send_error("network failure");
    let result = mock
        .send_message(channel(1), CreateMessage::new().content("x"))
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("network failure"));

    // Error is consumed — next call succeeds
    let result = mock
        .send_message(channel(1), CreateMessage::new().content("y"))
        .await;
    assert!(result.is_ok());
}

// --- MCP toggle persists to DB ---

#[tokio::test]
async fn toggle_mcp_persists_disabled_list() {
    let (sm, _, db) = setup();
    db.register_project("ch-1", "/tmp/p", "guild-1").unwrap();

    // toggle_mcp_server calls sdk_control (which needs an active session),
    // but the DB persist part runs unconditionally
    sm.toggle_mcp_server("ch-1", "server-a", false).await;
    let disabled = db.get_disabled_mcps("ch-1");
    assert_eq!(disabled, vec!["server-a"]);

    // Re-enable
    sm.toggle_mcp_server("ch-1", "server-a", true).await;
    let disabled = db.get_disabled_mcps("ch-1");
    assert!(disabled.is_empty());
}

// --- Shutdown and cleanup ---

#[tokio::test]
async fn shutdown_with_no_sessions_succeeds() {
    let (sm, _, _) = setup();
    sm.shutdown().await;
    // No panic = success
}

#[test]
fn cleanup_expired_pending_removes_nothing_when_empty() {
    let (sm, _, _) = setup();
    sm.cleanup_expired_pending();
    // No panic = success
}

#[tokio::test]
async fn toggle_mcp_no_duplicates() {
    let (sm, _, db) = setup();
    db.register_project("ch-1", "/tmp/p", "guild-1").unwrap();

    sm.toggle_mcp_server("ch-1", "s", false).await;
    sm.toggle_mcp_server("ch-1", "s", false).await;
    let disabled = db.get_disabled_mcps("ch-1");
    assert_eq!(disabled, vec!["s"]);
}
