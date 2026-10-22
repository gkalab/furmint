use super::*;
use crate::handlers::popup_create::{
    handle_create_directory_event, handle_create_file_event, handle_init_create_directory,
    handle_init_create_file,
};
use crate::ui::text_edit::TextEditState;
use termina::event::{KeyCode, Modifiers};

#[tokio::test]
async fn test_handle_create_directory_enter_empty_fails() {
    let mut app = basic_app_state().await;
    handle_init_create_directory(&mut app);
    let ret = handle_create_directory_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    // Should not accept empty name
    assert!(!ret);
    assert!(
        app.popups.create_directory.error.is_none()
            || app.popups.create_directory.new_name.text().is_empty()
    );
}

#[tokio::test]
async fn test_handle_create_file_errors() {
    let mut app = basic_app_state().await;
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    handle_init_create_file(&mut app);

    // Empty name
    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(app.popups.create_file.error.is_some());
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("empty")
    );

    // Existing file
    let temp_dir = std::env::temp_dir();
    let existing_file = temp_dir.join("fm_existing.txt");
    std::fs::File::create(&existing_file).unwrap();
    app.popups.create_file.parent_dir = temp_dir.clone();
    app.popups.create_file.input_value = TextEditState::with_text("fm_existing.txt");

    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(app.popups.create_file.error.is_some());
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("already exists")
    );

    std::fs::remove_file(&existing_file).ok();

    // Invalid name (ending with separator)
    let input = format!("some_dir{}", std::path::MAIN_SEPARATOR);
    app.popups.create_file.input_value = TextEditState::with_text(input);
    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(app.popups.create_file.error.is_some());
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("Invalid")
    );
}

#[tokio::test]
async fn test_handle_create_file_tilde_expansion() {
    let mut app = basic_app_state().await;
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    handle_init_create_file(&mut app);

    // Test ~ expansion (just check if it doesn't immediately fail with "Unsupported")
    app.popups.create_file.input_value = TextEditState::with_text("~");
    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    // It might fail because it's a directory, but shouldn't be "Unsupported ~username"
    if let Some(err) = &app.popups.create_file.error {
        assert!(!err.contains("Unsupported ~username"));
    }

    app.popups.create_file.input_value = TextEditState::with_text("~user");
    handle_create_file_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(
        app.popups
            .create_file
            .error
            .as_ref()
            .unwrap()
            .contains("Unsupported ~username")
    );
}
