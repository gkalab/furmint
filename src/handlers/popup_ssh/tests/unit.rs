use super::super::*;
use crate::ssh::history::SshConnectionInfo;
use ratatui::layout::Rect;
use termina::event::{MouseButton, MouseEvent, MouseEventKind};

fn left_click(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: Modifiers::NONE,
    }
}

/// App with the SSH popup open and one history entry whose connection
/// string fails validation ("user@" has an empty host), so Enter reports
/// an error instead of spawning a real connection.
fn ssh_popup_app() -> AppState {
    let mut app = crate::test_utils::create_test_app();
    app.ssh_history.add(SshConnectionInfo {
        name: None,
        connection_string: "user@".to_string(),
        user: "user".to_string(),
        host: String::new(),
        port: 22,
        path: None,
        sort_column: None,
        sort_direction: None,
    });
    app.popups
        .set_popup_visible(crate::app::PopupKind::SshConnection, true);
    app.layout.popups.ssh_connection.field_areas = vec![
        Rect::new(0, 0, 10, 1),
        Rect::new(0, 1, 10, 1),
        Rect::new(0, 2, 10, 1),
        Rect::new(0, 5, 10, 5),
    ];
    app
}

#[tokio::test]
async fn single_click_on_ssh_history_row_selects_without_connecting() {
    let mut app = ssh_popup_app();

    crate::handlers::mouse::handle_mouse_event(&mut app, left_click(2, 6)).await;

    assert_eq!(app.popups.ssh_connection.selected_history_idx, Some(0));
    assert_eq!(app.popups.ssh_connection.active_field, SshField::History);
    assert!(app.popups.ssh_connection.error.is_none());
    assert!(app.popups.ssh_connection.is_visible);
}

#[tokio::test]
async fn double_click_on_ssh_history_row_triggers_enter() {
    let mut app = ssh_popup_app();

    crate::handlers::mouse::handle_mouse_event(&mut app, left_click(2, 6)).await;
    crate::handlers::mouse::handle_mouse_event(&mut app, left_click(2, 6)).await;

    assert_eq!(
        app.popups.ssh_connection.error.as_deref(),
        Some("Invalid connection string format")
    );
}

#[test]
fn auth_failures_are_not_reported_as_task_failures() {
    use crate::ssh::manager::{AuthError, NetworkError, SshError};

    // A password prompt follows auth failures, so no failure message.
    assert_eq!(
        key_connect_task_status(&SshError::Auth(AuthError::NoAuthMethodsAvailable)),
        TaskStatus::Completed
    );
    assert_eq!(
        key_connect_task_status(&SshError::Auth(AuthError::KeyAuthFailed)),
        TaskStatus::Completed
    );
    assert_eq!(
        key_connect_task_status(&SshError::Auth(AuthError::AgentError(
            "agent died".to_string()
        ))),
        TaskStatus::Completed
    );

    // Other failures are surfaced with their message.
    assert_eq!(
        key_connect_task_status(&SshError::Network(NetworkError::ConnectionRefused)),
        TaskStatus::Failed("Network error: Connection refused".to_string())
    );
    assert_eq!(
        key_connect_task_status(&SshError::Connection("boom".to_string())),
        TaskStatus::Failed("Connection error: boom".to_string())
    );
}
