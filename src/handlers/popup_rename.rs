//! Rename popup event handler and helpers

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) fn handle_init_rename(app: &mut AppState) {
    let (current_dir, entry) = {
        let tab_manager = match app.active {
            crate::app::PanelSide::Left => &app.left,
            crate::app::PanelSide::Right => &app.right,
        };
        let panel = tab_manager.active_tab();
        (panel.current_dir.clone(), panel.current_entry().cloned())
    };

    if let Some(entry) = entry {
        if entry.name == ".." {
            return;
        }
        app.popups.rename.is_visible = true;
        app.popups.rename.original_name = entry.name.clone();
        app.popups.rename.new_name = entry.name.clone();
        app.popups.rename.parent_dir = current_dir;
        app.popups.rename.show_overwrite_confirm = false;
        app.popups.rename.is_dir = entry.is_dir;
        app.popups.rename.error = None;

        // Position cursor before extension
        let path = std::path::Path::new(&entry.name);
        if let Some(stem) = path.file_stem() {
            app.popups.rename.cursor_position = stem.len();
        } else {
            app.popups.rename.cursor_position = entry.name.len();
        }
    }
}

pub(crate) fn handle_rename_event(code: KeyCode, app: &mut AppState) -> bool {
    if app.popups.rename.show_overwrite_confirm {
        match code {
            KeyCode::Char('y' | 'Y') => {
                perform_rename(app, true);
                app.popups.rename.reset();
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.popups.rename.show_overwrite_confirm = false;
                app.popups.rename.reset();
            }
            _ => {}
        }
        return false;
    }

    match code {
        KeyCode::Esc => {
            app.popups.rename.reset();
        }
        KeyCode::Enter => {
            if app.popups.rename.new_name == app.popups.rename.original_name {
                app.popups.rename.reset();
            } else {
                let new_path = app
                    .popups
                    .rename
                    .parent_dir
                    .join(&app.popups.rename.new_name);
                if new_path.exists() {
                    if app.popups.rename.is_dir {
                        app.popups.rename.error =
                            Some("Error: Target directory exists".to_string());
                    } else {
                        app.popups.rename.show_overwrite_confirm = true;
                    }
                } else {
                    perform_rename(app, false);
                    app.popups.rename.reset();
                }
            }
        }
        KeyCode::Char(c) => {
            app.popups.rename.error = None; // Clear error on typing
            app.popups
                .rename
                .new_name
                .insert(app.popups.rename.cursor_position, c);
            app.popups.rename.cursor_position += 1;
        }
        KeyCode::Backspace => {
            app.popups.rename.error = None;
            if app.popups.rename.cursor_position > 0 {
                app.popups
                    .rename
                    .new_name
                    .remove(app.popups.rename.cursor_position - 1);
                app.popups.rename.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if app.popups.rename.cursor_position < app.popups.rename.new_name.len() {
                app.popups
                    .rename
                    .new_name
                    .remove(app.popups.rename.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.popups.rename.cursor_position > 0 {
                app.popups.rename.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.popups.rename.cursor_position < app.popups.rename.new_name.len() {
                app.popups.rename.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.popups.rename.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.rename.cursor_position = app.popups.rename.new_name.len();
        }
        _ => {}
    }
    false
}

pub(crate) fn perform_rename(app: &mut AppState, overwrite: bool) {
    let old_path = app
        .popups
        .rename
        .parent_dir
        .join(&app.popups.rename.original_name);
    let new_path = app
        .popups
        .rename
        .parent_dir
        .join(&app.popups.rename.new_name);

    let result = if overwrite && cfg!(target_os = "windows") && new_path.exists() {
        std::fs::remove_file(&new_path).and_then(|()| std::fs::rename(&old_path, &new_path))
    } else {
        std::fs::rename(&old_path, &new_path)
    };

    match result {
        Ok(()) => {
            let tab_manager = match app.active {
                crate::app::PanelSide::Left => &mut app.left,
                crate::app::PanelSide::Right => &mut app.right,
            };
            let panel = tab_manager.active_tab_mut();
            match crate::fs_ops::list_dir(&panel.current_dir) {
                Ok(entries) => {
                    panel.entries = entries;
                    panel.sort_entries();
                    if let Some(idx) = panel
                        .entries
                        .iter()
                        .position(|e| e.name == app.popups.rename.new_name)
                    {
                        panel.cursor = idx;
                    }
                }
                Err(e) => {
                    panel.error = Some(format!("Error refreshing directory: {e}"));
                }
            }
        }
        Err(e) => {
            let tab_manager = match app.active {
                crate::app::PanelSide::Left => &mut app.left,
                crate::app::PanelSide::Right => &mut app.right,
            };
            tab_manager.active_tab_mut().error = Some(format!("Error renaming: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, PanelSide, Tab};
    use crate::fs_ops::FileEntry;
    use crossterm::event::KeyCode;

    fn test_app_with_entry(name: &str, is_dir: bool, path: &std::path::Path) -> AppState {
        use crate::tasks::TaskEvent;
        use tokio::sync::mpsc;
        let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

        let mut tab = Tab::new(path).unwrap();
        tab.entries.clear();
        tab.entries.push(FileEntry {
            name: name.to_string(),
            is_dir,
            is_symlink: false,
            size: Some(123),
            modified: None,
            attributes: String::from("-rw-r--r--"),
            selected: false,
        });
        tab.cursor = 0;
        AppState {
            left: {
                let mut tm = crate::app::TabManager::new(path).unwrap();
                tm.tabs[0] = tab;
                tm
            },
            right: crate::app::TabManager::new(path).unwrap(),
            active: PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, ""),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(task_tx),
            task_decision_txs: Default::default(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
        }
    }

    #[test]
    fn test_init_rename_for_normal_file() {
        let mut app = test_app_with_entry("myfile.txt", false, &std::path::PathBuf::from("/tmp"));
        handle_init_rename(&mut app);
        assert!(app.popups.rename.is_visible);
        assert_eq!(app.popups.rename.original_name, "myfile.txt");
        // Cursor should be placed before extension
        assert!(app.popups.rename.cursor_position < app.popups.rename.original_name.len());
    }

    #[test]
    fn test_init_rename_skips_dotdot() {
        let mut app = test_app_with_entry("..", true, &std::path::PathBuf::from("/tmp"));
        handle_init_rename(&mut app);
        assert!(!app.popups.rename.is_visible);
    }

    #[test]
    fn test_rename_typing_and_backspace() {
        let mut app = test_app_with_entry("file.txt", false, &std::path::PathBuf::from("/tmp"));
        handle_init_rename(&mut app);
        let orig = app.popups.rename.new_name.clone();
        handle_rename_event(KeyCode::Char('a'), &mut app);
        assert_ne!(app.popups.rename.new_name, orig);
        handle_rename_event(KeyCode::Backspace, &mut app);
        assert_eq!(app.popups.rename.new_name, orig);
    }

    #[test]
    fn test_rename_esc_resets() {
        let mut app = test_app_with_entry("other.txt", false, &std::path::PathBuf::from("/tmp"));
        handle_init_rename(&mut app);
        assert!(app.popups.rename.is_visible);
        handle_rename_event(KeyCode::Esc, &mut app);
        assert!(!app.popups.rename.is_visible);
    }

    #[test]
    fn test_rename_enter_same_name_resets() {
        let mut app = test_app_with_entry("foo.txt", false, &std::path::PathBuf::from("/tmp"));
        handle_init_rename(&mut app);
        assert!(app.popups.rename.is_visible);
        handle_rename_event(KeyCode::Enter, &mut app);
        assert!(!app.popups.rename.is_visible);
    }

    #[test]
    fn test_rename_navigation() {
        let mut app = test_app_with_entry("test.txt", false, &std::path::PathBuf::from("/tmp"));
        handle_init_rename(&mut app);
        // Original name is test.txt, stem is test (len 4), cursor should be at 4
        assert_eq!(app.popups.rename.cursor_position, 4);

        handle_rename_event(KeyCode::Home, &mut app);
        assert_eq!(app.popups.rename.cursor_position, 0);

        handle_rename_event(KeyCode::End, &mut app);
        assert_eq!(app.popups.rename.cursor_position, 8); // test.txt len

        handle_rename_event(KeyCode::Left, &mut app);
        assert_eq!(app.popups.rename.cursor_position, 7);

        handle_rename_event(KeyCode::Delete, &mut app); // delete last 't'
        assert_eq!(app.popups.rename.new_name, "test.tx");
    }

    #[test]
    fn test_rename_overwrite_flow() {
        use std::fs::File;
        // Use tempfile for guaranteed isolation and cleanup
        let tmp_dir = tempfile::tempdir().unwrap();
        let temp_dir = tmp_dir.path();

        let file1 = temp_dir.join("file1.txt");
        let file2 = temp_dir.join("file2.txt");
        File::create(&file1).unwrap();
        File::create(&file2).unwrap();

        let mut app = test_app_with_entry("file1.txt", false, temp_dir);
        handle_init_rename(&mut app);

        // Rename file1 to file2
        app.popups.rename.new_name = "file2.txt".to_string();
        app.popups.rename.cursor_position = 9;

        handle_rename_event(KeyCode::Enter, &mut app);
        assert!(app.popups.rename.show_overwrite_confirm);

        handle_rename_event(KeyCode::Char('y'), &mut app);
        // On Unix, rename is usually successful.
        // We check if the popup was reset, which happens on success.
        assert!(!app.popups.rename.is_visible);

        assert!(file2.exists());
        assert!(!file1.exists());
    }
}
