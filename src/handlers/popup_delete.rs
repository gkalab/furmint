//! Delete popup event handler and deletion logic

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) fn handle_delete_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc | KeyCode::Char('n') => {
            app.popups.delete.reset();
        }
        KeyCode::Char('y') | KeyCode::Enter => {
            handle_confirm_delete(app);
            app.popups.delete.reset();
        }
        _ => {}
    }
    false
}

pub(crate) fn handle_init_delete(app: &mut AppState, permanent: bool) {
    let tab_manager = match app.active {
        crate::app::PanelSide::Left => &mut app.left,
        crate::app::PanelSide::Right => &mut app.right,
    };
    let tab = tab_manager.active_tab();
    let mut selected: Vec<_> = tab
        .get_selected_entries()
        .iter()
        .map(|e| tab.current_dir.join(&e.name))
        .collect();

    if selected.is_empty()
        && let Some(entry) = tab.current_entry()
        && entry.name != ".."
    {
        selected.push(tab.current_dir.join(&entry.name));
    }

    if selected.is_empty() {
        return;
    }

    app.popups.delete.selected_paths = selected;
    app.popups.delete.is_permanent = permanent;
    app.popups.delete.is_visible = true;
}

pub(crate) fn handle_confirm_delete(app: &mut AppState) {
    let paths = app.popups.delete.selected_paths.clone();
    let is_permanent = app.popups.delete.is_permanent;

    let name = if is_permanent {
        format!("Deleting {} items permanently", paths.len())
    } else {
        format!("Trashing {} items", paths.len())
    };

    app.task_manager
        .spawn_task(name, move |cancel, tx, id| async move {
            let total = paths.len();
            let mut success = 0;
            let mut failures = Vec::new();
            for (i, path) in paths.iter().enumerate() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Cancelled,
                    ));
                    return;
                }
                let result = if is_permanent {
                    if path.is_dir() {
                        std::fs::remove_dir_all(path)
                    } else {
                        std::fs::remove_file(path)
                    }
                    .map_err(|e| e.to_string())
                } else {
                    trash::delete(path).map_err(|e| e.to_string())
                };
                match result {
                    Ok(()) => success += 1,
                    Err(e) => failures.push(format!("{}: {}", path.display(), e)),
                }
                let _ = tx.send(crate::tasks::TaskEvent::UpdateProgress(id, i + 1, total));
            }
            if failures.is_empty() {
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Completed,
                ));
            } else {
                let error_msg = if success > 0 {
                    format!("Completed with errors: {} failed", failures.len())
                } else {
                    format!("Failed: {}", failures[0])
                };
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Failed(error_msg),
                ));
            }
        });

    // Clear selection in active tab if deletion started
    let tab_manager = match app.active {
        crate::app::PanelSide::Left => &mut app.left,
        crate::app::PanelSide::Right => &mut app.right,
    };
    for entry in &mut tab_manager.active_tab_mut().entries {
        entry.selected = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Tab;
    use crate::app::{AppState, PanelSide};
    use crate::fs_ops::FileEntry;
    use crossterm::event::KeyCode;
    use std::path::PathBuf;

    fn basic_app_with_entry(name: &str) -> AppState {
        use crate::state::FileViewerState;
        use crate::tasks::TaskEvent;
        use tokio::sync::mpsc;
        let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();

        let mut tab = Tab::new(&PathBuf::from("/tmp")).unwrap();
        tab.entries.clear();
        tab.entries.push(FileEntry {
            name: name.to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(42),
            modified: None,
            attributes: String::from("-rw-r--r--"),
            selected: false,
        });
        tab.cursor = 0;
        AppState {
            left: {
                let mut tm = crate::app::TabManager::new(PathBuf::from("/tmp")).unwrap();
                tm.tabs[0] = tab;
                tm
            },
            right: crate::app::TabManager::new(PathBuf::from("/tmp")).unwrap(),
            active: PanelSide::Left,
            file_viewer: FileViewerState::new(false, ""),
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
    fn test_handle_init_delete_populates_popup() {
        let mut app = basic_app_with_entry("test_file.txt");
        handle_init_delete(&mut app, false);
        assert!(app.popups.delete.is_visible);
        assert!(!app.popups.delete.selected_paths.is_empty());
        assert!(!app.popups.delete.is_permanent);
    }

    #[test]
    fn test_handle_init_delete_permanent_sets_flag() {
        let mut app = basic_app_with_entry("test_file2.txt");
        handle_init_delete(&mut app, true);
        assert!(app.popups.delete.is_visible);
        assert!(app.popups.delete.is_permanent);
    }

    #[test]
    fn test_handle_delete_event_esc_resets() {
        let mut app = basic_app_with_entry("will_reset.txt");
        handle_init_delete(&mut app, false);
        assert!(app.popups.delete.is_visible);
        handle_delete_event(KeyCode::Esc, &mut app);
        assert!(!app.popups.delete.is_visible);
    }

    #[tokio::test]
    async fn test_handle_delete_event_enter_triggers_confirm() {
        let mut app = basic_app_with_entry("some_file.txt");
        handle_init_delete(&mut app, false);
        assert!(app.popups.delete.is_visible);
        handle_delete_event(KeyCode::Enter, &mut app);
        // Should become invisible after
        assert!(!app.popups.delete.is_visible);
    }

    #[tokio::test]
    async fn test_handle_confirm_delete_clears_selection() {
        let mut app = basic_app_with_entry("file_to_del.txt");
        app.left.tabs[0].entries[0].selected = true;
        handle_init_delete(&mut app, true);
        handle_confirm_delete(&mut app);
        assert!(!app.left.tabs[0].entries[0].selected);
    }

    #[tokio::test]
    async fn test_handle_confirm_delete_multiple_files() {
        let mut app = basic_app_with_entry("file1.txt");
        app.left.tabs[0].entries.push(FileEntry {
            name: "file2.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: String::new(),
            selected: true,
        });
        app.left.tabs[0].entries[0].selected = true;
        handle_init_delete(&mut app, true);
        assert_eq!(app.popups.delete.selected_paths.len(), 2);
        handle_delete_event(KeyCode::Enter, &mut app);
        assert!(!app.popups.delete.is_visible);
    }
}
