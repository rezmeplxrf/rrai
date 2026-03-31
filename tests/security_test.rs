//! Tests for the security module (rate limiting, user allowlist).
//!
//! These tests require initializing the global config, so they use
//! a dedicated test that sets up the environment variables first.

use std::env;
use std::sync::Once;

static INIT: Once = Once::new();

fn init_config() {
    INIT.call_once(|| {
        // SAFETY: called once before any threads spawn, so no concurrent readers.
        unsafe {
            env::set_var("DISCORD_BOT_TOKEN", "test-token");
            env::set_var("DISCORD_GUILD_ID", "123456");
            env::set_var("ALLOWED_USER_IDS", "100,200,300");
            env::set_var("RATE_LIMIT_PER_MINUTE", "3");
        }
        rrai::config::load_config().expect("config should load");
    });
}

#[test]
fn allowed_user_returns_true() {
    init_config();
    assert!(rrai::security::is_allowed_user(100));
    assert!(rrai::security::is_allowed_user(200));
    assert!(rrai::security::is_allowed_user(300));
}

#[test]
fn disallowed_user_returns_false() {
    init_config();
    assert!(!rrai::security::is_allowed_user(999));
    assert!(!rrai::security::is_allowed_user(0));
}

#[test]
fn rate_limit_allows_up_to_limit() {
    init_config();
    // Use a unique user ID that won't collide with other tests
    let user_id = 9999;
    for _ in 0..3 {
        assert!(rrai::security::check_rate_limit(user_id));
    }
    // 4th call should be denied (limit is 3)
    assert!(!rrai::security::check_rate_limit(user_id));
}

#[test]
fn rate_limit_different_users_independent() {
    init_config();
    let user_a = 8001;
    let user_b = 8002;

    for _ in 0..3 {
        assert!(rrai::security::check_rate_limit(user_a));
    }
    // user_a exhausted, but user_b should still be fine
    assert!(rrai::security::check_rate_limit(user_b));
}

#[test]
fn cleanup_rate_limits_does_not_panic() {
    init_config();
    rrai::security::cleanup_rate_limits();
}
