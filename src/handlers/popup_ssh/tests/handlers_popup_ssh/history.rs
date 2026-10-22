use super::*;
use crate::handlers::popup_ssh::{
    handle_history_search, handle_ssh_connection_event, handle_ssh_connection_init,
};
use crate::state::ssh::SshField;
use termina::event::{KeyCode, Modifiers};

#[tokio::test]
async fn test_history_search_reset() {
    use crate::ssh::history::SshConnectionInfo;
    use std::time::Instant;
    let mut app = basic_app_state().await;
    handle_ssh_connection_init(&mut app);

    // Isolate from any real history loaded from the state directory.
    app.ssh_history.connections.clear();
    app.ssh_history.connections.push(SshConnectionInfo {
        name: Some("Target".to_string()),
        connection_string: "user@target".to_string(),
        user: "user".to_string(),
        host: "target".to_string(),
        port: 22,
        path: None,
        sort_column: None,
        sort_direction: None,
    });

    app.popups.ssh_connection.active_field = SshField::History;

    // Type 't'
    handle_ssh_connection_event(&mut app, KeyCode::Char('t'), Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.search_query, "t");
    assert_eq!(app.popups.ssh_connection.selected_history_idx, Some(0));

    // Wait is simulated by manually clearing or manipulating last_key_time
    // but the core logic can be tested by making a second call with a past instant
    app.popups.ssh_connection.last_key_time = Some(
        Instant::now()
            .checked_sub(std::time::Duration::from_secs(2))
            .unwrap(),
    );
    handle_history_search(&mut app, 'z');
    assert_eq!(app.popups.ssh_connection.search_query, "z");
}
