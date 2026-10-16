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

fn left_drag(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
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

#[tokio::test]
async fn single_click_on_ssh_field_sets_cursor() {
    let mut app = ssh_popup_app();
    app.popups
        .ssh_connection
        .connection_string
        .set_text("root@host");
    app.popups.ssh_connection.connection_string.set_cursor(0);

    // Field 0's text starts at x = 2; clicking x = 5 places the caret at char 3.
    crate::handlers::mouse::handle_mouse_event(&mut app, left_click(5, 0)).await;

    assert_eq!(
        app.popups.ssh_connection.active_field,
        SshField::ConnectionString
    );
    assert_eq!(app.popups.ssh_connection.connection_string.cursor(), 3);
    assert_eq!(
        app.mouse.active_drag,
        Some(crate::app::DragTarget::SshFieldSelection)
    );
}

#[tokio::test]
async fn drag_after_click_selects_ssh_field_text() {
    let mut app = ssh_popup_app();
    app.popups
        .ssh_connection
        .connection_string
        .set_text("abcdef");
    app.popups.ssh_connection.connection_string.set_cursor(0);

    // Click at column 1 (x = 3), then drag to column 5 (x = 7).
    crate::handlers::mouse::handle_mouse_event(&mut app, left_click(3, 0)).await;
    crate::handlers::mouse::handle_mouse_event(&mut app, left_drag(7, 0)).await;

    assert_eq!(
        app.popups
            .ssh_connection
            .connection_string
            .selected_text()
            .as_deref(),
        Some("bcde")
    );
}

#[tokio::test]
async fn clicking_another_field_clears_the_previous_selection() {
    let mut app = ssh_popup_app();
    app.popups
        .ssh_connection
        .connection_string
        .set_text("root@host");
    app.popups.ssh_connection.connection_string.select_all();

    // Click the Name field (row 1).
    crate::handlers::mouse::handle_mouse_event(&mut app, left_click(4, 1)).await;

    assert_eq!(app.popups.ssh_connection.active_field, SshField::Name);
    assert!(
        app.popups
            .ssh_connection
            .connection_string
            .selection()
            .is_none()
    );
}

#[tokio::test]
async fn click_on_password_field_sets_cursor() {
    let mut app = crate::test_utils::create_test_app();
    app.layout.screen = Rect::new(0, 0, 80, 24);
    app.popups
        .set_popup_visible(crate::app::PopupKind::SshPassword, true);
    app.popups.ssh_password.password.set_text("secret");
    app.popups.ssh_password.password.set_cursor(0);

    let (_, _, text_area) = crate::ui::ssh_ui::ssh_password_geometry(app.layout.screen);
    crate::handlers::mouse::handle_mouse_event(&mut app, left_click(text_area.x + 3, text_area.y))
        .await;

    assert_eq!(app.popups.ssh_password.password.cursor(), 3);
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
