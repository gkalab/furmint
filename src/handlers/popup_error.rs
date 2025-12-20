//! Error popup event handler

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) async fn handle_error_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    let task_id = app.error_popup.task_id;
    let decision = match code {
        KeyCode::Char('r' | 'R') => Some(crate::tasks::TaskDecision::Retry),
        KeyCode::Char('s' | 'S') => Some(crate::tasks::TaskDecision::Skip),
        KeyCode::Char('a' | 'A') => Some(crate::tasks::TaskDecision::SkipAll),
        KeyCode::Char('c' | 'C') | KeyCode::Esc => Some(crate::tasks::TaskDecision::Cancel),
        _ => None,
    };
    if let Some(d) = decision {
        if let Some(tx) = app.task_decision_txs.get(&task_id) {
            let _ = tx.send(d).await;
        }
        app.error_popup.reset();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::state::ErrorState;
    use crate::tasks::{TaskDecision, TaskManager};
    use crossterm::event::KeyCode;
    use std::collections::HashMap;
    // use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn test_handle_error_popup_event_retry_and_cancel() {
        let mut app = AppState {
            left: crate::app::TabManager {
                tabs: vec![],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            rename_popup: crate::state::RenameState::new(),
            create_directory_popup: crate::state::CreateDirectoryState::new(),
            delete_popup: crate::state::DeleteState::new(),
            empty_trash_popup: crate::state::EmptyTrashState::new(),
            copy_move_popup: crate::state::CopyMoveState::new(),
            conflict_popup: crate::state::ConflictState::new(),
            error_popup: ErrorState {
                is_visible: true,
                task_id: 42,
                error_path: String::new(),
                error_message: String::new(),
            },
            quit_confirmation: crate::state::QuitConfirmationState::new(),
            task_manager: TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            create_file_popup: crate::state::CreateFileState::new(),
            help_popup: crate::state::HelpState::new(),
            drive_select_popup: crate::state::DriveSelectState::new(),
            task_decision_txs: HashMap::new(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(2);
        app.task_decision_txs.insert(42, tx);
        // Send retry
        handle_error_popup_event(KeyCode::Char('r'), &mut app).await;
        assert!(!app.error_popup.is_visible);
        use tokio::runtime::Handle;
        let decision = Handle::current().block_on(rx.recv()).unwrap();
        assert_eq!(decision, TaskDecision::Retry);
        // Reset state for cancel test
        app.error_popup.is_visible = true;
        let (tx2, _rx2) = tokio::sync::mpsc::channel(2);
        app.task_decision_txs.insert(42, tx2);
        handle_error_popup_event(KeyCode::Esc, &mut app).await;
        assert!(!app.error_popup.is_visible);
    }
}
