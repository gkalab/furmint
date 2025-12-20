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
    if panel.entries.is_empty() {
        app.file_viewer.content = vec![];
        return;
    }
    let entry = &panel.entries[panel.cursor];
    if entry.is_dir {
        app.file_viewer.content = vec!["Directory".to_string()];
    } else {
        let full_path = panel.current_dir.join(&entry.name);
        app.file_viewer.load_content(full_path);
    }
}

// Enter directory or try opening file
pub(crate) fn handle_enter_directory(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Some(entry) = panel.current_entry().cloned() {
        if entry.is_dir {
            let new_dir = if entry.name == ".." {
                panel.current_dir.parent().map(std::path::Path::to_path_buf)
            } else {
                Some(panel.current_dir.join(&entry.name))
            };

            if let Some(path) = new_dir {
                if let Err(e) = panel.navigate_to(path.clone()) {
                    panel.error = Some(format!("Error: {e}"));
                } else {
                    app.dir_history.record_visit(&path);
                    update_viewer_content(app);
                }
            }
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
    use crate::state::{
        ConflictState, CopyMoveState, CreateDirectoryState, CreateFileState, DeleteState,
        DriveSelectState, EmptyTrashState, ErrorState, FileViewerState, HelpState,
        QuitConfirmationState, RenameState,
    };
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
            rename_popup: RenameState::new(),
            create_directory_popup: CreateDirectoryState::new(),
            delete_popup: DeleteState::new(),
            empty_trash_popup: EmptyTrashState::new(),
            copy_move_popup: CopyMoveState::new(),
            conflict_popup: ConflictState::new(),
            error_popup: ErrorState::new(),
            quit_confirmation: QuitConfirmationState::new(),
            task_manager: TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            create_file_popup: CreateFileState::new(),
            help_popup: HelpState::new(),
            drive_select_popup: DriveSelectState::new(),
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
}
