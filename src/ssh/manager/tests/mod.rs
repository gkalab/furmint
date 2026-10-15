use super::*;
use secrecy::ExposeSecret;

#[test]
fn test_backoff_values_and_jitter_bounds() {
    let mgr = SshManager {
        base_backoff: Duration::from_secs(1),
        backoff_factor: 2.0,
        max_backoff: std::time::Duration::from_mins(1),
        jitter_pct: 0.0,
        keepalive_interval: 10,
        read_timeout_secs: 15,
        connect_timeout_secs: 30,
        sessions: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        password_cache: std::sync::Arc::new(std::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        known_hosts: std::sync::Arc::new(crate::ssh::known_hosts::KnownHosts::with_path(
            std::path::PathBuf::from("/tmp/fm_test_known_hosts_backoff"),
        )),
    };

    assert_eq!(mgr.compute_backoff(1).as_secs(), 1);
    assert_eq!(mgr.compute_backoff(2).as_secs(), 2);
    assert_eq!(mgr.compute_backoff(3).as_secs(), 4);
    assert_eq!(mgr.compute_backoff(8).as_secs(), 60);

    let mgr_j = SshManager {
        jitter_pct: 0.2,
        ..mgr
    };

    let d = mgr_j.compute_backoff(3);
    let expected = 4.0;
    let low = expected * (1.0 - 0.2);
    let high = expected * (1.0 + 0.2);
    let got = d.as_secs_f64();
    assert!(
        got >= low && got <= high,
        "got {got} not in [{low}..{high}]"
    );

    // Jittered delay at the backoff cap must never exceed the cap.
    for _ in 0..100 {
        assert!(mgr_j.compute_backoff(8).as_secs_f64() <= 60.0);
        assert!(mgr_j.compute_backoff(8).as_secs_f64() >= 60.0 * (1.0 - 0.2));
    }
}

#[test]
fn test_generate_session_id() {
    let id1 = SshManager::generate_session_id("example.com", 22);
    assert!(id1.starts_with("ssh_example.com_22_"));
    // Verify it contains a timestamp-like suffix
    let suffix = &id1["ssh_example.com_22_".len()..];
    assert!(
        suffix.parse::<u64>().is_ok(),
        "suffix should be numeric: {suffix}"
    );
}

#[test]
fn test_session_registration_and_tracking() {
    let mgr = SshManager::new(None);

    // Register a session
    mgr.register_session(
        "session_test".to_string(),
        "example.com".to_string(),
        22,
        "user".to_string(),
        Some("/remote/path".to_string()),
        AuthMethod::Password,
    );

    // Verify session is registered
    let session = mgr.get_session("session_test");
    assert!(session.is_some());
    let session = session.unwrap();
    assert_eq!(session.host, "example.com");
    assert_eq!(session.port, 22);
    assert_eq!(session.user, "user");
    assert_eq!(session.auth_method, AuthMethod::Password);
    assert!(!session.reconnecting);

    // Reconnecting flag toggles and reads back
    mgr.set_reconnecting("session_test", true);
    assert!(mgr.get_session("session_test").unwrap().reconnecting);
    mgr.set_reconnecting("session_test", false);
    assert!(!mgr.get_session("session_test").unwrap().reconnecting);
    // Flag for an unknown session is a no-op
    mgr.set_reconnecting("unknown", true);

    // Unregister session
    mgr.unregister_session("session_test");
    assert!(mgr.get_session("session_test").is_none());
}

#[test]
fn test_get_session_distinguishes_same_user_host() {
    let mgr = SshManager::new(None);

    mgr.register_session(
        "ssh_host1.com_22_1".to_string(),
        "host1.com".to_string(),
        22,
        "user".to_string(),
        Some("/path/a".to_string()),
        AuthMethod::Password,
    );
    mgr.register_session(
        "ssh_host1.com_22_2".to_string(),
        "host1.com".to_string(),
        22,
        "user".to_string(),
        Some("/path/b".to_string()),
        AuthMethod::Pubkey,
    );

    assert_eq!(
        mgr.get_session("ssh_host1.com_22_1")
            .unwrap()
            .target_path
            .as_deref(),
        Some("/path/a")
    );
    assert_eq!(
        mgr.get_session("ssh_host1.com_22_2")
            .unwrap()
            .target_path
            .as_deref(),
        Some("/path/b")
    );
}

#[test]
fn test_get_all_sessions() {
    let mgr = SshManager::new(None);

    mgr.register_session(
        "s1".to_string(),
        "host1.com".to_string(),
        22,
        "u1".to_string(),
        None,
        AuthMethod::Password,
    );
    mgr.register_session(
        "s2".to_string(),
        "host2.com".to_string(),
        22,
        "u2".to_string(),
        None,
        AuthMethod::Pubkey,
    );

    let sessions = mgr.get_all_sessions();
    assert_eq!(sessions.len(), 2);
}

#[test]
fn test_password_cache() {
    let mgr = SshManager::new(None);

    let session_id = "test_session";
    let password = "secret123";

    // Initially no cached password
    assert!(mgr.get_cached_password(session_id).is_none());

    mgr.cache_password(session_id, SecretString::new(password.to_string().into()));
    assert_eq!(
        mgr.get_cached_password(session_id)
            .map(|p| p.expose_secret().to_string()),
        Some(password.to_string())
    );

    // Clear password
    mgr.clear_password(session_id);
    assert!(mgr.get_cached_password(session_id).is_none());

    // Test with a different session to verify cache works independently
    let session_id2 = "test_session2";
    let password2 = "secret456";

    // Initially no cached password
    assert!(mgr.get_cached_password(session_id2).is_none());

    // Cache password
    mgr.cache_password(session_id2, SecretString::new(password2.to_string().into()));
    assert_eq!(
        mgr.get_cached_password(session_id2)
            .map(|p| p.expose_secret().to_string()),
        Some(password2.to_string())
    );

    // Clear password for session_id2
    mgr.clear_password(session_id2);
    assert!(mgr.get_cached_password(session_id2).is_none());

    mgr.cache_password(session_id, SecretString::new(password.to_string().into()));
    assert_eq!(
        mgr.get_cached_password(session_id)
            .map(|p| p.expose_secret().to_string()),
        Some(password.to_string())
    );

    // Clear password
    mgr.clear_password(session_id);
    assert!(mgr.get_cached_password(session_id).is_none());

    // Test multiple sessions
    mgr.cache_password("session1", SecretString::new("pass1".to_string().into()));
    mgr.cache_password("session2", SecretString::new("pass2".to_string().into()));
    assert_eq!(
        mgr.get_cached_password("session1")
            .map(|p| p.expose_secret().to_string()),
        Some("pass1".to_string())
    );
    assert_eq!(
        mgr.get_cached_password("session2")
            .map(|p| p.expose_secret().to_string()),
        Some("pass2".to_string())
    );

    // Clear all passwords
    mgr.clear_all_passwords();
    assert!(mgr.get_cached_password("session1").is_none());
    assert!(mgr.get_cached_password("session2").is_none());
}
