//! Handlers for creating files and directories in popup

use crate::app::AppState;
use crate::handlers::clipboard_utils::{
    get_clipboard_content, insert_text_at_cursor, insert_text_at_cursor_unicode,
};
use crossterm::event::{KeyCode, KeyModifiers};

pub fn handle_init_create_file(app: &mut AppState) {
    let parent_dir = app.active_tab().current_dir.clone();
    app.popups.create_file.is_visible = true;
    app.popups.create_file.input_value.clear();
    app.popups.create_file.cursor_position = 0;
    app.popups.create_file.error = None;
    app.popups.create_file.parent_dir = parent_dir;
}

pub fn handle_init_create_directory(app: &mut AppState) {
    app.popups.create_directory.is_visible = true;
    app.popups.create_directory.new_name.clear();
    app.popups.create_directory.cursor_position = 0;
    app.popups.create_directory.error = None;
}

pub fn handle_create_directory_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut AppState,
) -> bool {
    match code {
        KeyCode::Esc => {
            app.popups.create_directory.is_visible = false;
            app.popups.create_directory.reset();
        }
        KeyCode::Enter => {
            let new_name = app.popups.create_directory.new_name.trim().to_string();
            if new_name.is_empty() {
                return false;
            }

            let current_dir = app.active_tab().current_dir.clone();
            let new_path = current_dir.join(&new_name);

            let result = app.active_tab_mut().provider.create_dir(&new_path);
            match result {
                Ok(()) => {
                    app.popups.create_directory.is_visible = false;
                    app.popups.create_directory.reset();
                    // Reload active tab
                    let (entries, panel_cursor_update) = {
                        let panel = app.active_tab_mut();
                        let entries = panel.provider.list_dir(&current_dir).ok();
                        let mut new_cursor = None;
                        if let Some(entries) = &entries
                            && let Some(name) = new_path.file_name().and_then(|n| n.to_str())
                            && let Some(idx) = entries.iter().position(|e| e.name == name)
                        {
                            new_cursor = Some(idx);
                        }
                        (entries, new_cursor)
                    };

                    if let Some(entries) = entries {
                        let panel = app.active_tab_mut();
                        panel.entries = entries;
                        panel.sort_entries();
                        if let Some(idx) = panel_cursor_update {
                            panel.cursor = idx;
                        }
                    }
                }
                Err(e) => {
                    app.popups.create_directory.error = Some(e.to_string());
                }
            }
        }
        KeyCode::Backspace => {
            if app.popups.create_directory.cursor_position > 0 {
                let current_len = app.popups.create_directory.new_name.chars().count();
                if app.popups.create_directory.cursor_position <= current_len {
                    // Remove char at cursor_position - 1
                    let byte_idx = app
                        .popups
                        .create_directory
                        .new_name
                        .char_indices()
                        .nth(app.popups.create_directory.cursor_position - 1)
                        .map(|(i, _)| i)
                        .unwrap();
                    app.popups.create_directory.new_name.remove(byte_idx);
                    app.popups.create_directory.cursor_position -= 1;
                }
            }
        }
        KeyCode::Delete => {
            let current_len = app.popups.create_directory.new_name.chars().count();
            if app.popups.create_directory.cursor_position < current_len {
                let byte_idx = app
                    .popups
                    .create_directory
                    .new_name
                    .char_indices()
                    .nth(app.popups.create_directory.cursor_position)
                    .map(|(i, _)| i)
                    .unwrap();
                app.popups.create_directory.new_name.remove(byte_idx);
            }
        }
        KeyCode::Left => {
            if app.popups.create_directory.cursor_position > 0 {
                app.popups.create_directory.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            let len = app.popups.create_directory.new_name.chars().count();
            if app.popups.create_directory.cursor_position < len {
                app.popups.create_directory.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.popups.create_directory.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.create_directory.cursor_position =
                app.popups.create_directory.new_name.chars().count();
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(content) = get_clipboard_content() {
                insert_text_at_cursor_unicode(
                    &mut app.popups.create_directory.new_name,
                    &mut app.popups.create_directory.cursor_position,
                    &content,
                );
            }
        }
        KeyCode::Char(c) => {
            let idx = app.popups.create_directory.cursor_position;
            if idx >= app.popups.create_directory.new_name.chars().count() {
                app.popups.create_directory.new_name.push(c);
            } else {
                let byte_idx = app
                    .popups
                    .create_directory
                    .new_name
                    .char_indices()
                    .nth(idx)
                    .map(|(i, _)| i)
                    .unwrap();
                app.popups.create_directory.new_name.insert(byte_idx, c);
            }
            app.popups.create_directory.cursor_position += 1;
        }
        _ => {}
    }
    // Clear error on any input in the popup
    if code != KeyCode::Enter && app.popups.create_directory.error.is_some() {
        app.popups.create_directory.error = None;
    }
    false
}

pub async fn handle_create_file_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut AppState,
    input_tx: &tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> bool {
    use std::path::Path;
    match code {
        KeyCode::Esc => {
            app.popups.create_file.reset();
        }
        KeyCode::Enter => {
            app.popups.create_file.error = None;
            let input = app.popups.create_file.input_value.trim();
            if input.is_empty() {
                app.popups.create_file.error = Some("File name cannot be empty".to_string());
                return false;
            }
            let path_buf = if input.starts_with('~') {
                if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
                {
                    if input == "~" {
                        home
                    } else if let Some(stripped) = input.strip_prefix("~/") {
                        home.join(stripped)
                    } else {
                        app.popups.create_file.error =
                            Some("Unsupported ~username syntax".to_string());
                        return false;
                    }
                } else {
                    app.popups.create_file.error =
                        Some("Cannot resolve ~ to home directory".to_string());
                    return false;
                }
            } else {
                let p = Path::new(input);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    app.popups.create_file.parent_dir.join(p)
                }
            };
            // Disallow creating a directory and special names
            let provider = app.active_tab().provider.clone();
            if path_buf.as_os_str().is_empty()
                || input.ends_with('/')
                || input.ends_with(std::path::MAIN_SEPARATOR)
                || provider.is_dir(&path_buf)
            {
                app.popups.create_file.error = Some("Invalid file name".to_string());
                return false;
            }
            // File must not already exist
            if provider.exists(&path_buf) {
                app.popups.create_file.error =
                    Some("A file with that name already exists".to_string());
                return false;
            }
            // Try to create empty file
            if let Err(e) = provider.create_file(&path_buf) {
                app.popups.create_file.error = Some(format!("Failed to create file: {e}"));
                return false;
            }
            // Open in editor using environment helper
            let file_name_opt = path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .map(std::string::ToString::to_string);
            let editor_result = crate::handlers::editor::open_file_in_editor_with_env_handling(
                app,
                &path_buf,
                file_name_opt,
                input_tx,
            )
            .await;
            if let Err(e) = editor_result {
                app.popups.create_file.error = Some(format!("Failed to open in editor: {e}"));
                return false;
            }
            app.popups.create_file.reset();
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(content) = get_clipboard_content() {
                insert_text_at_cursor(
                    &mut app.popups.create_file.input_value,
                    &mut app.popups.create_file.cursor_position,
                    &content,
                );
            }
        }
        KeyCode::Char(c) => {
            app.popups.create_file.error = None;
            app.popups
                .create_file
                .input_value
                .insert(app.popups.create_file.cursor_position, c);
            app.popups.create_file.cursor_position += 1;
        }
        KeyCode::Backspace => {
            app.popups.create_file.error = None;
            if app.popups.create_file.cursor_position > 0 {
                app.popups
                    .create_file
                    .input_value
                    .remove(app.popups.create_file.cursor_position - 1);
                app.popups.create_file.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            app.popups.create_file.error = None;
            if app.popups.create_file.cursor_position < app.popups.create_file.input_value.len() {
                app.popups
                    .create_file
                    .input_value
                    .remove(app.popups.create_file.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.popups.create_file.cursor_position > 0 {
                app.popups.create_file.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.popups.create_file.cursor_position < app.popups.create_file.input_value.len() {
                app.popups.create_file.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.popups.create_file.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.create_file.cursor_position = app.popups.create_file.input_value.len();
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, PanelSide};
    use crossterm::event::KeyCode;
    use std::path::Path;

    fn basic_app_state() -> AppState {
        use crate::state::FileViewerState;
        use crate::tasks::TaskEvent;
        use tokio::sync::mpsc;

        let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

        AppState {
            left: crate::app::TabManager::new(Path::new("/tmp")).unwrap(),
            right: crate::app::TabManager::new(Path::new("/tmp")).unwrap(),
            active: PanelSide::Left,
            file_viewer: FileViewerState::new(false, ""),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(task_tx),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: Default::default(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::InMemoryFileClipboard::new()),
        }
    }

    #[test]
    fn test_handle_init_create_file_and_directory() {
        let mut app = basic_app_state();
        app.active = PanelSide::Right;
        handle_init_create_file(&mut app);
        assert!(app.popups.create_file.is_visible);
        assert_eq!(app.popups.create_file.input_value, "");
        assert_eq!(app.popups.create_file.cursor_position, 0);
        assert!(app.popups.create_file.error.is_none());

        handle_init_create_directory(&mut app);
        assert!(app.popups.create_directory.is_visible);
        assert_eq!(app.popups.create_directory.new_name, "");
        assert_eq!(app.popups.create_directory.cursor_position, 0);
        assert!(app.popups.create_directory.error.is_none());
    }

    #[test]
    fn test_handle_create_directory_event_typing_backspace() {
        let mut app = basic_app_state();
        handle_init_create_directory(&mut app);
        // A typical typing workflow
        for c in "abc".chars() {
            handle_create_directory_event(KeyCode::Char(c), KeyModifiers::NONE, &mut app);
        }
        assert_eq!(app.popups.create_directory.new_name, "abc");
        assert_eq!(app.popups.create_directory.cursor_position, 3);
        // Backspace -- removes one char
        handle_create_directory_event(KeyCode::Backspace, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.create_directory.new_name, "ab");
        assert_eq!(app.popups.create_directory.cursor_position, 2);
    }

    #[test]
    fn test_handle_create_directory_enter_empty_fails() {
        let mut app = basic_app_state();
        handle_init_create_directory(&mut app);
        let ret = handle_create_directory_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);
        // Should not accept empty name
        assert!(!ret);
        assert!(
            app.popups.create_directory.error.is_none()
                || app.popups.create_directory.new_name.is_empty()
        );
    }

    #[test]
    fn test_handle_create_directory_navigation() {
        let mut app = basic_app_state();
        handle_init_create_directory(&mut app);
        for c in "abcd".chars() {
            handle_create_directory_event(KeyCode::Char(c), KeyModifiers::NONE, &mut app);
        }
        assert_eq!(app.popups.create_directory.cursor_position, 4);

        handle_create_directory_event(KeyCode::Left, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.create_directory.cursor_position, 3);

        handle_create_directory_event(KeyCode::Right, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.create_directory.cursor_position, 4);

        handle_create_directory_event(KeyCode::Home, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.create_directory.cursor_position, 0);

        handle_create_directory_event(KeyCode::End, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.create_directory.cursor_position, 4);

        handle_create_directory_event(KeyCode::Left, KeyModifiers::NONE, &mut app); // at pos 3
        handle_create_directory_event(KeyCode::Left, KeyModifiers::NONE, &mut app); // at pos 2
        handle_create_directory_event(KeyCode::Delete, KeyModifiers::NONE, &mut app); // delete 'c'
        assert_eq!(app.popups.create_directory.new_name, "abd");
    }

    #[tokio::test]
    async fn test_handle_create_file_event() {
        let mut app = basic_app_state();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_init_create_file(&mut app);

        handle_create_file_event(KeyCode::Char('f'), KeyModifiers::NONE, &mut app, &tx).await;
        assert_eq!(app.popups.create_file.input_value, "f");

        handle_create_file_event(KeyCode::Backspace, KeyModifiers::NONE, &mut app, &tx).await;
        assert_eq!(app.popups.create_file.input_value, "");

        handle_create_file_event(KeyCode::Esc, KeyModifiers::NONE, &mut app, &tx).await;
        assert!(!app.popups.create_file.is_visible);
    }

    #[tokio::test]
    async fn test_handle_create_file_errors() {
        let mut app = basic_app_state();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_init_create_file(&mut app);

        // Empty name
        handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
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
        app.popups.create_file.input_value = "fm_existing.txt".to_string();
        app.popups.create_file.cursor_position = 15;

        handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
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
        app.popups.create_file.input_value = input.clone();
        app.popups.create_file.cursor_position = input.len();
        handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
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
        let mut app = basic_app_state();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_init_create_file(&mut app);

        // Test ~ expansion (just check if it doesn't immediately fail with "Unsupported")
        app.popups.create_file.input_value = "~".to_string();
        app.popups.create_file.cursor_position = 1;
        handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
        // It might fail because it's a directory, but shouldn't be "Unsupported ~username"
        if let Some(err) = &app.popups.create_file.error {
            assert!(!err.contains("Unsupported ~username"));
        }

        app.popups.create_file.input_value = "~user".to_string();
        app.popups.create_file.cursor_position = 5;
        handle_create_file_event(KeyCode::Enter, KeyModifiers::NONE, &mut app, &tx).await;
        assert!(
            app.popups
                .create_file
                .error
                .as_ref()
                .unwrap()
                .contains("Unsupported ~username")
        );
    }

    #[test]
    fn test_handle_create_file_navigation() {
        let mut app = basic_app_state();
        handle_init_create_file(&mut app);
        app.popups.create_file.input_value = "test.txt".to_string();
        app.popups.create_file.cursor_position = 8;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();

        rt.block_on(async {
            handle_create_file_event(KeyCode::Left, KeyModifiers::NONE, &mut app, &tx).await;
            assert_eq!(app.popups.create_file.cursor_position, 7);

            handle_create_file_event(KeyCode::Right, KeyModifiers::NONE, &mut app, &tx).await;
            assert_eq!(app.popups.create_file.cursor_position, 8);

            handle_create_file_event(KeyCode::Home, KeyModifiers::NONE, &mut app, &tx).await;
            assert_eq!(app.popups.create_file.cursor_position, 0);

            handle_create_file_event(KeyCode::End, KeyModifiers::NONE, &mut app, &tx).await;
            assert_eq!(app.popups.create_file.cursor_position, 8);

            handle_create_file_event(KeyCode::Delete, KeyModifiers::NONE, &mut app, &tx).await; // nothing to delete at end
            assert_eq!(app.popups.create_file.input_value, "test.txt");

            app.popups.create_file.cursor_position = 0;
            handle_create_file_event(KeyCode::Delete, KeyModifiers::NONE, &mut app, &tx).await;
            assert_eq!(app.popups.create_file.input_value, "est.txt");
        });
    }
}
