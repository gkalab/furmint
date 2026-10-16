use super::*;

#[test]
fn password_show_populates_and_makes_visible() {
    let mut state = SshPasswordState::new();
    state.password.set_text("stale");
    state.show(
        "sid".to_string(),
        "host".to_string(),
        "user".to_string(),
        Some("bad password".to_string()),
    );

    assert!(state.is_visible);
    assert_eq!(state.session_id, "sid");
    assert_eq!(state.host, "host");
    assert_eq!(state.user, "user");
    assert_eq!(state.error.as_deref(), Some("bad password"));
    assert!(state.password.is_empty());
    assert_eq!(state.password.cursor(), 0);
}

#[test]
fn connection_show_error_populates_and_makes_visible() {
    let mut state = SshConnectionState::new();
    state.active_field = SshField::History;
    state.show_error("no route to host".to_string());

    assert!(state.is_visible);
    assert_eq!(state.error.as_deref(), Some("no route to host"));
    assert_eq!(state.active_field, SshField::ConnectionString);
}
