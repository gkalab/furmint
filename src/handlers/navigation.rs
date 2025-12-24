//! Navigation-related event handlers for directory and panel navigation.

use crate::app::{AppState, PanelSide};

// Moves the cursor up in the active panel.
pub(crate) fn handle_up(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_up();
    update_viewer_content(app);
}

// Moves the cursor down in the active panel.
pub(crate) fn handle_down(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_down();
    update_viewer_content(app);
}

// Moves the cursor a page up.
pub(crate) fn handle_page_up(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_page_up(20);
    update_viewer_content(app);
}

// Moves the cursor a page down.
pub(crate) fn handle_page_down(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_page_down(20);
    update_viewer_content(app);
}

// Moves the cursor to the home position.
pub(crate) fn handle_home(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_home();
    update_viewer_content(app);
}

// Moves the cursor to the end.
pub(crate) fn handle_end(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_end();
    update_viewer_content(app);
}

// Handles quick type-to-select in the active panel.
pub(crate) fn handle_type_char(app: &mut AppState, c: char) {
    use std::time::Instant;
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();
    let now = Instant::now();
    let reset_threshold = std::time::Duration::from_secs(1);
    // If last_type_time is None or too old, reset buffer
    if panel
        .last_type_time
        .is_none_or(|t| now.duration_since(t) > reset_threshold)
    {
        panel.typed_buffer.clear();
    }
    panel.typed_buffer.push(c);
    panel.last_type_time = Some(now);
    let typed = panel.typed_buffer.to_lowercase();
    // Find first entry whose name starts with typed
    if let Some((idx, _)) = panel
        .entries
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name.to_lowercase().starts_with(&typed))
    {
        panel.cursor = idx;
        update_viewer_content(app);
    }
}

// Switches between panels or focuses file viewer.
pub(crate) fn handle_tab(app: &mut AppState) {
    if app.file_viewer.is_visible {
        app.file_viewer.focused = true;
    } else {
        app.active = match app.active {
            PanelSide::Left => PanelSide::Right,
            PanelSide::Right => PanelSide::Left,
        };
    }
}

/// Update content in external file viewer based on cursor.
pub(crate) fn update_viewer_content(app: &mut AppState) {
    if !app.file_viewer.is_visible {
        return;
    }
    let tab_manager = match app.active {
        PanelSide::Left => &app.left,
        PanelSide::Right => &app.right,
    };
    let panel = tab_manager.active_tab();
    let Some(entry) = panel.current_entry() else {
        app.file_viewer.content = vec![];
        return;
    };
    if entry.is_dir {
        app.file_viewer.content = vec!["Directory".to_string()];
    } else {
        let full_path = panel.current_dir.join(&entry.name);
        app.file_viewer.load_content(full_path);
    }
}

// Enter directory
pub(crate) fn handle_enter_directory(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Some(entry) = panel.current_entry().cloned()
        && entry.is_dir
    {
        let new_dir = if entry.name == ".." {
            panel.current_dir.parent().map(std::path::Path::to_path_buf)
        } else {
            Some(panel.current_dir.join(&entry.name))
        };

        if let Some(path) = new_dir {
            if let Err(e) = panel.navigate_to(&path) {
                panel.error = Some(format!("Error: {e}"));
            } else {
                app.dir_history.record_visit(&path);
                update_viewer_content(app);
            }
        }
    }
}

// Enter directory or try opening file
pub(crate) fn handle_open_item(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Some(entry) = panel.current_entry().cloned() {
        if entry.is_dir {
            handle_enter_directory(app);
        } else {
            let full_path = panel.current_dir.join(&entry.name);
            let is_exe = crate::fs_ops::is_executable(&full_path, &entry);

            if is_exe {
                // Launch executable in the default terminal
                let configured_terminal = app.global.terminal.clone();
                if let Err(e) = crate::handlers::terminal::spawn_terminal(
                    &panel.current_dir,
                    configured_terminal,
                    vec![full_path.to_string_lossy().to_string()],
                    false,
                ) {
                    panel.error = Some(format!("Error launching in terminal: {e}"));
                }
            } else {
                #[cfg(target_os = "linux")]
                {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&full_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
                #[cfg(not(target_os = "linux"))]
                {
                    if let Err(e) = open::that(&full_path) {
                        panel.error = Some(format!("Error opening file: {}", e));
                    }
                }
            }
        }
    }
}

pub(crate) fn handle_directory_up(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Some(parent) = panel.current_dir.parent() {
        let parent_path = parent.to_path_buf();
        if let Err(e) = panel.go_up() {
            panel.error = Some(format!("Error: {e}"));
        } else {
            app.dir_history.record_visit(&parent_path);
            update_viewer_content(app);
        }
    }
}

pub(crate) fn handle_history_previous(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Err(e) = panel.go_back() {
        panel.error = Some(format!("Error: {e}"));
    } else {
        app.dir_history.record_visit(&panel.current_dir);
        update_viewer_content(app);
    }
}

pub(crate) fn handle_history_next(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Err(e) = panel.go_forward() {
        panel.error = Some(format!("Error: {e}"));
    } else {
        app.dir_history.record_visit(&panel.current_dir);
        update_viewer_content(app);
    }
}

pub(crate) fn handle_sort(app: &mut AppState, column: crate::app::SortColumn) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().handle_sort(column);
    update_viewer_content(app);
}

pub(crate) fn handle_toggle_selection(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().toggle_selection();
    update_viewer_content(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, PanelSide, Tab, TabManager};
    use crate::config::GlobalConfig;
    use crate::dir_history::DirectoryHistory;
    use crate::fs_ops::FileEntry;
    use crate::state::FileViewerState;
    use crate::tasks::TaskManager;

    fn test_app(entries: Vec<FileEntry>) -> AppState {
        let tab = Tab {
            current_dir: std::path::PathBuf::from("/tmp"),
            entries,
            cursor: 0,
            history: vec![],
            history_index: 0,
            error: None,
            typed_buffer: String::new(),
            last_type_time: None,
            sort_column: crate::app::SortColumn::Name,
            sort_direction: crate::app::SortDirection::Ascending,
            scroll_offset: 0,
        };
        AppState {
            left: TabManager {
                tabs: vec![tab.clone()],
                active_tab_index: 0,
            },
            right: TabManager {
                tabs: vec![tab],
                active_tab_index: 0,
            },
            active: PanelSide::Left,
            file_viewer: FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
        }
    }

    #[test]
    fn test_handle_up_moves_cursor() {
        let entry = FileEntry {
            name: "one".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        };
        let mut app = test_app(vec![entry.clone(); 3]);
        app.left.active_tab_mut().cursor = 2;
        handle_up(&mut app);
        assert_eq!(app.left.active_tab().cursor, 1);
    }

    #[test]
    fn test_handle_down_moves_cursor() {
        let entry = FileEntry {
            name: "one".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        };
        let mut app = test_app(vec![entry.clone(); 3]);
        handle_down(&mut app);
        assert_eq!(app.left.active_tab().cursor, 1);
    }

    #[test]
    fn test_handle_page_up_down() {
        let entries = vec![
            FileEntry {
                name: "file".to_string(),
                is_dir: false,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: String::new(),
                selected: false
            };
            50
        ];
        let mut app = test_app(entries);
        app.left.active_tab_mut().cursor = 45;
        handle_page_up(&mut app);
        assert!(app.left.active_tab().cursor < 45);
        handle_page_down(&mut app);
        assert!(app.left.active_tab().cursor > 0);
    }

    #[test]
    fn test_handle_home_end() {
        let entries = vec![
            FileEntry {
                name: "file".to_string(),
                is_dir: false,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: String::new(),
                selected: false
            };
            10
        ];
        let mut app = test_app(entries);
        app.left.active_tab_mut().cursor = 4;
        handle_home(&mut app);
        assert_eq!(app.left.active_tab().cursor, 0);
        handle_end(&mut app);
        assert_eq!(app.left.active_tab().cursor, 9);
    }

    #[test]
    fn test_handle_enter_directory_only_enters_dirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();
        let dir_path = path.join("test_dir");
        std::fs::create_dir(&dir_path).unwrap();
        let file_path = path.join("test_file");
        std::fs::write(&file_path, "test").unwrap();

        let mut app = test_app(vec![]);
        app.left.active_tab_mut().current_dir = path.to_path_buf();
        // Manually list to populate entries
        app.left
            .active_tab_mut()
            .navigate_to(&path.to_path_buf())
            .unwrap();

        // Find file and dir indices
        let file_idx = app
            .left
            .active_tab()
            .entries
            .iter()
            .position(|e| e.name == "test_file")
            .unwrap();
        let dir_idx = app
            .left
            .active_tab()
            .entries
            .iter()
            .position(|e| e.name == "test_dir")
            .unwrap();

        // Try to enter a file
        app.left.active_tab_mut().cursor = file_idx;
        let original_dir = app.left.active_tab().current_dir.clone();
        handle_enter_directory(&mut app);
        assert_eq!(
            app.left.active_tab().current_dir,
            original_dir,
            "Should NOT enter a file"
        );

        // Try to enter a directory
        app.left.active_tab_mut().cursor = dir_idx;
        handle_enter_directory(&mut app);
        assert_ne!(
            app.left.active_tab().current_dir,
            original_dir,
            "Should enter a directory"
        );
        assert!(app.left.active_tab().current_dir.ends_with("test_dir"));

        // Test handle_open_item on file (should stay in same dir as it spawns process)
        app.left
            .active_tab_mut()
            .navigate_to(&path.to_path_buf())
            .unwrap();
        app.left.active_tab_mut().cursor = file_idx;
        let original_dir = app.left.active_tab().current_dir.clone();
        handle_open_item(&mut app);
        assert_eq!(
            app.left.active_tab().current_dir,
            original_dir,
            "handle_open_item on file should not change directory"
        );

        // Test handle_open_item on directory (should enter)
        app.left.active_tab_mut().cursor = dir_idx;
        handle_open_item(&mut app);
        assert_ne!(
            app.left.active_tab().current_dir,
            original_dir,
            "handle_open_item on directory should change directory"
        );
        assert!(app.left.active_tab().current_dir.ends_with("test_dir"));
    }

    #[test]
    fn test_handle_type_char() {
        let entries = vec![
            FileEntry {
                name: "apple".to_string(),
                is_dir: false,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: String::new(),
                selected: false,
            },
            FileEntry {
                name: "banana".to_string(),
                is_dir: false,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: String::new(),
                selected: false,
            },
        ];
        let mut app = test_app(entries);

        handle_type_char(&mut app, 'b');
        assert_eq!(app.left.active_tab().cursor, 1);

        // Test buffer reset (we can't easily wait 1s in unit test, but we can check it appends)
        handle_type_char(&mut app, 'a');
        // 'ba' doesn't match anything, so cursor should stay at 1 (previous match)
        assert_eq!(app.left.active_tab().cursor, 1);

        app.left.active_tab_mut().typed_buffer.clear();
        handle_type_char(&mut app, 'a');
        assert_eq!(app.left.active_tab().cursor, 0);
    }

    #[test]
    fn test_handle_tab() {
        let mut app = test_app(vec![]);
        assert_eq!(app.active, PanelSide::Left);

        handle_tab(&mut app);
        assert_eq!(app.active, PanelSide::Right);

        handle_tab(&mut app);
        assert_eq!(app.active, PanelSide::Left);

        // Test focusing file viewer
        app.file_viewer.is_visible = true;
        app.file_viewer.focused = false;
        handle_tab(&mut app);
        assert!(app.file_viewer.focused);
    }

    #[test]
    fn test_handle_sort_and_toggle() {
        let entries = vec![FileEntry {
            name: "a".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        }];
        let mut app = test_app(entries);

        handle_toggle_selection(&mut app);
        assert!(app.left.active_tab().entries[0].selected);

        handle_sort(&mut app, crate::app::SortColumn::Size);
        assert_eq!(
            app.left.active_tab().sort_column,
            crate::app::SortColumn::Size
        );
    }

    #[test]
    fn test_handle_history_and_directory_up() {
        // This test is limited because go_up/go_back require actual filesystem or complex mocking
        // But we can at least call them to see they don't panic and cover the handler lines.
        let mut app = test_app(vec![]);
        handle_directory_up(&mut app);
        handle_history_previous(&mut app);
        handle_history_next(&mut app);
    }
}
