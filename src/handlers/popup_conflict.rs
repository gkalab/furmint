//! Conflict popup event handler

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) async fn handle_conflict_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    let task_id = app.conflict_popup.task_id;
    let decision = match code {
        KeyCode::Char('o' | 'O') => Some(crate::tasks::TaskDecision::Overwrite),
        KeyCode::Char('s' | 'S') => Some(crate::tasks::TaskDecision::Skip),
        KeyCode::Char('c' | 'C') | KeyCode::Esc => Some(crate::tasks::TaskDecision::Cancel),
        KeyCode::Char('y' | 'Y') => Some(crate::tasks::TaskDecision::OverwriteAll),
        KeyCode::Char('a' | 'A' | 'n' | 'N') => Some(crate::tasks::TaskDecision::SkipAll),
        KeyCode::Char('m' | 'M') => Some(crate::tasks::TaskDecision::Merge),
        _ => None,
    };

    if let Some(d) = decision {
        if let Some(tx) = app.task_decision_txs.get(&task_id) {
            let _ = tx.send(d).await;
        }
        app.conflict_popup.reset();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::state::ConflictState;
    use crossterm::event::KeyCode;
    use crate::tasks::TaskDecision;
    use tokio::sync::mpsc;
    use std::collections::HashMap;

    fn app_with_conflict_popup(task_id: usize) -> (AppState, mpsc::Receiver<TaskDecision>) {
        use crate::state::*;
        use crate::tasks::TaskEvent;
        let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();
        let (dec_tx, dec_rx) = mpsc::channel(1);
        let mut app = AppState {
            left: crate::app::TabManager::new(std::path::PathBuf::from("/tmp")).unwrap(),
            right: crate::app::TabManager::new(std::path::PathBuf::from("/tmp")).unwrap(),
            active: crate::app::PanelSide::Left,
            file_viewer: FileViewerState::new(false, ""),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            rename_popup: RenameState::default(),
            create_directory_popup: CreateDirectoryState::default(),
            delete_popup: DeleteState::default(),
            empty_trash_popup: EmptyTrashState::default(),
            copy_move_popup: CopyMoveState::default(),
            conflict_popup: ConflictState {
                is_visible: true,
                task_id,
                conflict_path: std::path::PathBuf::from("/tmp/fake"),
                conflict_type: crate::tasks::ConflictType::FileExists,
            },
            error_popup: ErrorState::default(),
            quit_confirmation: QuitConfirmationState::default(),
            task_manager: crate::tasks::TaskManager::new(task_tx),
            create_file_popup: CreateFileState::default(),
            help_popup: HelpState::default(),
            drive_select_popup: DriveSelectState::default(),
            task_decision_txs: {
                let mut map = HashMap::new();
                map.insert(task_id, dec_tx);
                map
            },
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
        };
        (app, dec_rx)
    }

    #[tokio::test]
    async fn sends_decision_and_resets() {
        let task_id = 123;
        let mapping = [
            (KeyCode::Char('o'), TaskDecision::Overwrite),
            (KeyCode::Char('O'), TaskDecision::Overwrite),
            (KeyCode::Char('s'), TaskDecision::Skip),
            (KeyCode::Char('S'), TaskDecision::Skip),
            (KeyCode::Char('c'), TaskDecision::Cancel),
            (KeyCode::Char('C'), TaskDecision::Cancel),
            (KeyCode::Char('y'), TaskDecision::OverwriteAll),
            (KeyCode::Char('Y'), TaskDecision::OverwriteAll),
            (KeyCode::Char('a'), TaskDecision::SkipAll),
            (KeyCode::Char('A'), TaskDecision::SkipAll),
            (KeyCode::Char('n'), TaskDecision::SkipAll),
            (KeyCode::Char('N'), TaskDecision::SkipAll),
            (KeyCode::Char('m'), TaskDecision::Merge),
            (KeyCode::Char('M'), TaskDecision::Merge),
            (KeyCode::Esc, TaskDecision::Cancel),
        ];
        for (key, expected_decision) in mapping.iter().cloned() {
            let (mut app, mut rx) = app_with_conflict_popup(task_id);
            let still_visible = app.conflict_popup.is_visible;
            assert!(still_visible);
            let _ = handle_conflict_popup_event(key, &mut app).await;
            let recv = rx.try_recv().unwrap();
            assert_eq!(recv, expected_decision);
            assert!(!app.conflict_popup.is_visible);
        }
    }

    #[tokio::test]
    async fn ignores_irrelevant_keys() {
        let (mut app, mut rx) = app_with_conflict_popup(42);
        let _ = handle_conflict_popup_event(KeyCode::Char('z'), &mut app).await;
        assert!(app.conflict_popup.is_visible, "Unmapped key should not reset popup");
        assert!(rx.try_recv().is_err(), "No decision should be sent for unmapped key");
    }
}
