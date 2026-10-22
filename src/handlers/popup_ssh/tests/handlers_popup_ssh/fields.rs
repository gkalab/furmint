use super::*;
use crate::handlers::popup_ssh::{handle_ssh_connection_event, handle_ssh_connection_init};
use crate::state::ssh::SshField;
use termina::event::{KeyCode, Modifiers};

#[tokio::test]
async fn test_ssh_field_cycling() {
    let mut app = basic_app_state().await;
    handle_ssh_connection_init(&mut app);

    assert_eq!(
        app.popups.ssh_connection.active_field,
        SshField::ConnectionString
    );

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.active_field, SshField::Name);

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.active_field, SshField::Port);

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.active_field, SshField::History);

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);
    assert_eq!(
        app.popups.ssh_connection.active_field,
        SshField::ConnectionString
    );

    handle_ssh_connection_event(&mut app, KeyCode::BackTab, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.active_field, SshField::History);
}

#[tokio::test]
async fn shift_home_selects_to_the_start_of_the_ssh_field() {
    let mut app = basic_app_state().await;
    handle_ssh_connection_init(&mut app);
    app.popups
        .ssh_connection
        .connection_string
        .set_text("root@host");

    // The field is focused with the caret at the end; Shift+Home selects back
    // to the start. This exercises the full key-routing path.
    handle_ssh_connection_event(&mut app, KeyCode::Home, Modifiers::SHIFT);

    assert_eq!(
        app.popups.ssh_connection.connection_string.selection(),
        Some(0..9)
    );
}

#[tokio::test]
async fn tab_clears_the_selection_in_the_field_being_left() {
    let mut app = basic_app_state().await;
    handle_ssh_connection_init(&mut app);
    app.popups
        .ssh_connection
        .connection_string
        .set_text("root@host");
    app.popups.ssh_connection.connection_string.select_all();
    assert!(
        app.popups
            .ssh_connection
            .connection_string
            .selection()
            .is_some()
    );

    handle_ssh_connection_event(&mut app, KeyCode::Tab, Modifiers::NONE);

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
async fn test_ssh_editing_cursor_movement() {
    let mut app = basic_app_state().await;
    handle_ssh_connection_init(&mut app);
    app.popups
        .ssh_connection
        .connection_string
        .set_text("root@host");
    app.popups.ssh_connection.connection_string.set_cursor(9);

    handle_ssh_connection_event(&mut app, KeyCode::Left, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string.cursor(), 8);

    handle_ssh_connection_event(&mut app, KeyCode::Home, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string.cursor(), 0);

    handle_ssh_connection_event(&mut app, KeyCode::End, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string.cursor(), 9);
}

#[tokio::test]
async fn test_ssh_insert_delete() {
    let mut app = basic_app_state().await;
    handle_ssh_connection_init(&mut app);
    app.popups.ssh_connection.connection_string.set_text("host");
    app.popups.ssh_connection.connection_string.set_cursor(0);

    // Insert at start
    handle_ssh_connection_event(&mut app, KeyCode::Char('a'), Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string.text(), "ahost");
    assert_eq!(app.popups.ssh_connection.connection_string.cursor(), 1);

    // Delete at position 1 (deletes 'h')
    handle_ssh_connection_event(&mut app, KeyCode::Delete, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string.text(), "aost");
    assert_eq!(app.popups.ssh_connection.connection_string.cursor(), 1);

    // Backspace (deletes 'a')
    handle_ssh_connection_event(&mut app, KeyCode::Backspace, Modifiers::NONE);
    assert_eq!(app.popups.ssh_connection.connection_string.text(), "ost");
    assert_eq!(app.popups.ssh_connection.connection_string.cursor(), 0);
}
