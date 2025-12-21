//! Copy/move popup handler and spawning logic

use crate::app::{AppState, PanelSide};
use crossterm::event::KeyCode;

pub fn handle_init_copy(app: &mut AppState) {
    init_copy_move(app, crate::app::CopyMoveAction::Copy);
}

pub fn handle_init_move(app: &mut AppState) {
    init_copy_move(app, crate::app::CopyMoveAction::Move);
}

pub fn init_copy_move(app: &mut AppState, action: crate::app::CopyMoveAction) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let tab = tab_manager.active_tab();
    let selected: Vec<_> = tab
        .get_selected_entries()
        .iter()
        .map(|e| tab.current_dir.join(&e.name))
        .collect();

    let paths = if selected.is_empty() {
        if let Some(entry) = tab.current_entry() {
            if entry.name == ".." {
                vec![]
            } else {
                vec![tab.current_dir.join(&entry.name)]
            }
        } else {
            vec![]
        }
    } else {
        selected
    };

    if paths.is_empty() {
        return;
    }

    // Get inactive panel path
    let inactive_tab = match app.active {
        PanelSide::Left => app.right.active_tab(),
        PanelSide::Right => app.left.active_tab(),
    };
    let dest = inactive_tab.current_dir.to_string_lossy().to_string();

    app.popups.copy_move.source_paths = paths;
    app.popups.copy_move.action = action;
    app.popups.copy_move.destination_input = dest;
    app.popups.copy_move.cursor_position = app.popups.copy_move.destination_input.len();
    app.popups.copy_move.input_selected = false;
    app.popups.copy_move.is_visible = true;
}

pub fn handle_copy_move_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.popups.copy_move.reset();
        }
        KeyCode::Enter => {
            let dest_input = app.popups.copy_move.destination_input.clone();
            let dest_path = if dest_input.starts_with('~') {
                if let Some(base_dirs) = directories::BaseDirs::new() {
                    let home = base_dirs.home_dir();
                    if dest_input == "~" {
                        home.to_path_buf()
                    } else {
                        home.join(dest_input.trim_start_matches("~/"))
                    }
                } else {
                    std::path::PathBuf::from(dest_input)
                }
            } else {
                std::path::PathBuf::from(dest_input)
            };
            let dest_abs = if let Ok(p) = dest_path.canonicalize() {
                p
            } else if dest_path.is_absolute() {
                dest_path
            } else {
                match app.active {
                    PanelSide::Left => app.left.active_tab().current_dir.join(&dest_path),
                    PanelSide::Right => app.right.active_tab().current_dir.join(&dest_path),
                }
            };
            app.popups.copy_move.destination_input = dest_abs.to_string_lossy().to_string();
            for src in &app.popups.copy_move.source_paths {
                if let Ok(src_abs) = src.canonicalize() {
                    if src_abs == dest_abs {
                        app.popups.copy_move.error =
                            Some("Cannot copy/move source into itself".to_string());
                        return false;
                    }
                    if dest_abs.starts_with(&src_abs) {
                        app.popups.copy_move.error =
                            Some("Cannot copy/move into subdirectory of itself".to_string());
                        return false;
                    }
                    if let Some(file_name) = src_abs.file_name() {
                        let effective_dest = dest_abs.join(file_name);
                        if effective_dest == src_abs {
                            app.popups.copy_move.error =
                                Some("Source and destination are the same".to_string());
                            return false;
                        }
                    }
                }
            }
            spawn_copy_move_task(app);
            app.popups.copy_move.reset();
        }
        KeyCode::Char(c) => {
            app.popups.copy_move.error = None;
            app.popups
                .copy_move
                .destination_input
                .insert(app.popups.copy_move.cursor_position, c);
            app.popups.copy_move.cursor_position += 1;
        }
        KeyCode::Backspace => {
            if app.popups.copy_move.cursor_position > 0 {
                app.popups
                    .copy_move
                    .destination_input
                    .remove(app.popups.copy_move.cursor_position - 1);
                app.popups.copy_move.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if app.popups.copy_move.cursor_position < app.popups.copy_move.destination_input.len() {
                app.popups
                    .copy_move
                    .destination_input
                    .remove(app.popups.copy_move.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.popups.copy_move.cursor_position > 0 {
                app.popups.copy_move.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.popups.copy_move.cursor_position < app.popups.copy_move.destination_input.len() {
                app.popups.copy_move.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.popups.copy_move.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.copy_move.cursor_position = app.popups.copy_move.destination_input.len();
        }
        _ => {}
    }
    false
}

pub fn spawn_copy_move_task(app: &mut AppState) {
    let paths = app.popups.copy_move.source_paths.clone();
    let dest_str = app.popups.copy_move.destination_input.clone();
    let action = app.popups.copy_move.action;

    // Validate destination
    let dest_path = std::path::PathBuf::from(&dest_str);

    let task_name = match action {
        crate::app::CopyMoveAction::Copy => format!("Copying {} items", paths.len()),
        crate::app::CopyMoveAction::Move => format!("Moving {} items", paths.len()),
    };

    // Deselect files in active panel
    {
        let entries = match app.active {
            crate::app::PanelSide::Left => &mut app.left.active_tab_mut().entries,
            crate::app::PanelSide::Right => &mut app.right.active_tab_mut().entries,
        };
        for entry in entries.iter_mut() {
            if entry.selected {
                entry.selected = false;
            }
        }
    }

    // Create channel for decisions
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(1);

    let id = app
        .task_manager
        .spawn_task(task_name, move |cancel, tx, id| async move {
            // Pre-calculation of total items (approximate)
            let total_items = crate::handlers::file_ops::count_items(&paths).await;
            let processed_items = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

            // State for "Apply to all" decisions
            // We use a struct to hold this state across recursions
            let mut decision_state = crate::handlers::file_ops::DecisionState {
                overwrite_all: false,
                skip_all: false,

                last_update: std::time::Instant::now(),
            };

            // We need `decision_rx` to be mutual, so we wrap it
            let decision_rx = std::sync::Arc::new(tokio::sync::Mutex::new(decision_rx));

            // Ensure dest dir exists if multiple items or if treated as dir
            let treat_as_dir = paths.len() > 1
                || dest_path.is_dir()
                || dest_str.ends_with(std::path::MAIN_SEPARATOR);

            if treat_as_dir {
                if let Err(e) = tokio::fs::create_dir_all(&dest_path).await {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Failed(e.to_string()),
                    ));
                    return;
                }
            } else if let Some(parent) = dest_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            let mut failures = Vec::new();

            for src in &paths {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }

                let file_name = match src.file_name() {
                    Some(n) => n,
                    None => continue,
                };

                let target = if treat_as_dir {
                    dest_path.join(file_name)
                } else {
                    dest_path.clone()
                };

                // Recursive copy/move
                let ctx = crate::handlers::file_ops::RecursiveOpContext {
                    tx: &tx,
                    id,
                    total: total_items,
                    processed: &processed_items,
                    decision_rx: &decision_rx,
                };
                let fs_impl = crate::handlers::file_ops::StdFileSystem;
                let res = crate::handlers::file_ops::recursive_op(
                    &fs_impl,
                    src,
                    &target,
                    action,
                    &cancel,
                    ctx,
                    &mut decision_state,
                )
                .await;

                if let Err(e) = res {
                    failures.push(e);
                }
            }

            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Cancelled,
                ));
            } else if failures.is_empty() {
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Completed,
                ));
            } else {
                // ... error handling
                let msg = format!("Failed with {} errors", failures.len());
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Failed(msg),
                ));
            }
        });

    // Store decision tx
    app.task_decision_txs.insert(id, decision_tx);
}

#[cfg(test)]
mod popup_copy_move_unit_tests {
    use super::*;

    use crate::app::{AppState, PanelSide, Tab, TabManager};
    use crate::fs_ops::FileEntry;
    use crate::state::CopyMoveAction;
    use crossterm::event::KeyCode;
    use std::collections::HashMap;
    use std::path::PathBuf;

    // --- Helpers to build minimal AppState for popup tests ---

    fn make_fileentry(name: &str, selected: bool, is_dir: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            is_dir,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected,
        }
    }

    fn make_tab(path: &str, entries: Vec<FileEntry>, cursor: usize) -> Tab {
        Tab {
            current_dir: PathBuf::from(path),
            entries,
            cursor,
            history: vec![],
            history_index: 0,
            error: None,
            typed_buffer: String::new(),
            last_type_time: None,
            sort_column: crate::app::SortColumn::Name,
            sort_direction: crate::app::SortDirection::Ascending,
            scroll_offset: 0,
        }
    }

    fn make_tab_manager(tab: Tab) -> TabManager {
        TabManager {
            tabs: vec![tab],
            active_tab_index: 0,
        }
    }

    fn minimal_state_with_entries(
        active: PanelSide,
        left_entries: Vec<FileEntry>,
        right_entries: Vec<FileEntry>,
        left_cursor: usize,
        right_cursor: usize,
    ) -> AppState {
        AppState {
            left: make_tab_manager(make_tab("/left", left_entries, left_cursor)),
            right: make_tab_manager(make_tab("/right", right_entries, right_cursor)),
            active,
            // Popups and config fields as default/minimal:
            file_viewer: Default::default(),
            fuzzy_search: Default::default(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            task_decision_txs: HashMap::new(),
            show_task_manager: false,
            dir_history: Default::default(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: Default::default(),
            editor_cfg: Default::default(),
            viewer_cfg: Default::default(),
        }
    }

    #[test]
    fn test_init_copy_and_move_selects_correct_paths() {
        // Left panel active, selected entry (not '..'), should populate paths
        let left_entries = vec![
            make_fileentry("A.txt", true, false),
            make_fileentry("..", false, true),
        ];
        let right_entries = vec![make_fileentry("X", false, false)];
        let mut app =
            minimal_state_with_entries(PanelSide::Left, left_entries, right_entries.clone(), 0, 0);
        handle_init_copy(&mut app);
        assert_eq!(app.popups.copy_move.is_visible, true);
        assert_eq!(app.popups.copy_move.action, CopyMoveAction::Copy);
        assert!(app.popups.copy_move.source_paths[0].ends_with("A.txt"));

        let left_entries = vec![
            make_fileentry("B.txt", false, false),
            make_fileentry("..", false, true),
        ];
        let mut app =
            minimal_state_with_entries(PanelSide::Left, left_entries, right_entries.clone(), 0, 0);
        handle_init_move(&mut app);
        assert_eq!(app.popups.copy_move.action, CopyMoveAction::Move);
    }

    #[test]
    fn test_init_copy_for_no_selection_uses_current_if_not_parent() {
        let left_entries = vec![
            make_fileentry("foo", false, false),
            make_fileentry("..", false, true),
        ];
        // Cursor points to "foo"
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        assert!(app.popups.copy_move.source_paths[0].ends_with("foo"));
    }

    #[test]
    fn test_init_copy_for_parent_dir_does_nothing() {
        let left_entries = vec![make_fileentry("..", false, true)];
        // Cursor points to ".."
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        assert!(!app.popups.copy_move.is_visible);
        assert_eq!(app.popups.copy_move.source_paths.len(), 0);
    }

    #[test]
    fn test_handle_copy_move_event_char_and_edit() {
        let left_entries = vec![make_fileentry("a", true, false)];
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        app.popups.copy_move.destination_input.clear();
        app.popups.copy_move.cursor_position = 0;
        // Insert 'x'
        handle_copy_move_event(KeyCode::Char('x'), &mut app);
        assert_eq!(app.popups.copy_move.destination_input, "x");
        assert_eq!(app.popups.copy_move.cursor_position, 1);
        // Insert 'y' at position 1
        handle_copy_move_event(KeyCode::Char('y'), &mut app);
        assert_eq!(app.popups.copy_move.destination_input, "xy");
        assert_eq!(app.popups.copy_move.cursor_position, 2);
        // Backspace
        handle_copy_move_event(KeyCode::Backspace, &mut app);
        assert_eq!(app.popups.copy_move.destination_input, "x");
        assert_eq!(app.popups.copy_move.cursor_position, 1);
        // Left
        handle_copy_move_event(KeyCode::Left, &mut app);
        assert_eq!(app.popups.copy_move.cursor_position, 0);
        // Delete (removes 'x')
        handle_copy_move_event(KeyCode::Delete, &mut app);
        assert_eq!(app.popups.copy_move.destination_input, "");
        assert_eq!(app.popups.copy_move.cursor_position, 0);
    }

    #[test]
    fn test_handle_copy_move_event_navigation_keys() {
        let left_entries = vec![make_fileentry("a", true, false)];
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        app.popups.copy_move.destination_input = "abcdef".to_string();
        app.popups.copy_move.cursor_position = 3;
        // Home
        handle_copy_move_event(KeyCode::Home, &mut app);
        assert_eq!(app.popups.copy_move.cursor_position, 0);
        // End
        handle_copy_move_event(KeyCode::End, &mut app);
        assert_eq!(app.popups.copy_move.cursor_position, 6);
        // Right at end (should stay)
        handle_copy_move_event(KeyCode::Right, &mut app);
        assert_eq!(app.popups.copy_move.cursor_position, 6);
    }

    #[test]
    fn test_handle_copy_move_event_escape_resets_popup() {
        let left_entries = vec![make_fileentry("a", true, false)];
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        app.popups.copy_move.error = Some("some error".to_string());
        assert!(app.popups.copy_move.is_visible);
        handle_copy_move_event(KeyCode::Esc, &mut app);
        assert!(!app.popups.copy_move.is_visible);
        assert!(app.popups.copy_move.error.is_none());
    }
}
