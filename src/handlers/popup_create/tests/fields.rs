use super::*;
use crate::app_state::tabs::PanelSide;
use crate::handlers::popup_create::{
    handle_create_directory_event, handle_create_file_event, handle_init_create_directory,
    handle_init_create_file,
};
use crate::ui::text_edit::TextEditState;
use termina::event::{KeyCode, Modifiers};

#[tokio::test]
async fn test_handle_init_create_file_and_directory() {
    let mut app = basic_app_state().await;
    app.panels.active = PanelSide::Right;
    handle_init_create_file(&mut app);
    assert!(app.popups.create_file.is_visible);
    assert_eq!(app.popups.create_file.input_value.text(), "");
    assert_eq!(app.popups.create_file.input_value.cursor(), 0);
    assert!(app.popups.create_file.error.is_none());

    handle_init_create_directory(&mut app);
    assert!(app.popups.create_directory.is_visible);
    assert_eq!(app.popups.create_directory.new_name.text(), "");
    assert_eq!(app.popups.create_directory.new_name.cursor(), 0);
    assert!(app.popups.create_directory.error.is_none());
}

#[tokio::test]
async fn test_handle_create_directory_event_typing_backspace() {
    let mut app = basic_app_state().await;
    handle_init_create_directory(&mut app);
    // A typical typing workflow
    for c in "abc".chars() {
        handle_create_directory_event(KeyCode::Char(c), Modifiers::NONE, &mut app).await;
    }
    assert_eq!(app.popups.create_directory.new_name.text(), "abc");
    assert_eq!(app.popups.create_directory.new_name.cursor(), 3);
    // Backspace -- removes one char
    handle_create_directory_event(KeyCode::Backspace, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_directory.new_name.text(), "ab");
    assert_eq!(app.popups.create_directory.new_name.cursor(), 2);
}

#[tokio::test]
async fn test_handle_create_directory_navigation() {
    let mut app = basic_app_state().await;
    handle_init_create_directory(&mut app);
    for c in "abcd".chars() {
        handle_create_directory_event(KeyCode::Char(c), Modifiers::NONE, &mut app).await;
    }
    assert_eq!(app.popups.create_directory.new_name.cursor(), 4);

    handle_create_directory_event(KeyCode::Left, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_directory.new_name.cursor(), 3);

    handle_create_directory_event(KeyCode::Right, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_directory.new_name.cursor(), 4);

    handle_create_directory_event(KeyCode::Home, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_directory.new_name.cursor(), 0);

    handle_create_directory_event(KeyCode::End, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_directory.new_name.cursor(), 4);

    handle_create_directory_event(KeyCode::Left, Modifiers::NONE, &mut app).await; // at pos 3
    handle_create_directory_event(KeyCode::Left, Modifiers::NONE, &mut app).await; // at pos 2
    handle_create_directory_event(KeyCode::Delete, Modifiers::NONE, &mut app).await; // delete 'c'
    assert_eq!(app.popups.create_directory.new_name.text(), "abd");
}

#[tokio::test]
async fn test_handle_create_file_event() {
    let mut app = basic_app_state().await;
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    handle_init_create_file(&mut app);

    handle_create_file_event(KeyCode::Char('f'), Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value.text(), "f");

    handle_create_file_event(KeyCode::Backspace, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value.text(), "");

    handle_create_file_event(KeyCode::Escape, Modifiers::NONE, &mut app).await;
    assert!(!app.popups.create_file.is_visible);
}

#[tokio::test]
async fn test_handle_create_file_navigation() {
    let mut app = basic_app_state().await;
    handle_init_create_file(&mut app);
    app.popups.create_file.input_value = TextEditState::with_text("test.txt");

    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();

    handle_create_file_event(KeyCode::Left, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value.cursor(), 7);

    handle_create_file_event(KeyCode::Right, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value.cursor(), 8);

    handle_create_file_event(KeyCode::Home, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value.cursor(), 0);

    handle_create_file_event(KeyCode::End, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value.cursor(), 8);

    handle_create_file_event(KeyCode::Delete, Modifiers::NONE, &mut app).await; // nothing to delete at end
    assert_eq!(app.popups.create_file.input_value.text(), "test.txt");

    app.popups.create_file.input_value.set_cursor(0);
    handle_create_file_event(KeyCode::Delete, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.create_file.input_value.text(), "est.txt");
}
