//! Conflict popup event handler

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) async fn handle_conflict_event(code: KeyCode, app: &mut AppState) -> bool {
    let task_id = app.popups.conflict.task_id;
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
        app.popups.conflict.reset();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::tasks::TaskDecision;
    use crossterm::event::KeyCode;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    fn app_with_conflict(task_id: usize) -> (AppState, mpsc::Receiver<TaskDecision>) {
        use crate::state::*;
        use crate::tasks::TaskEvent;
        let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();
        let (dec_tx, dec_rx) = mpsc::channel(1);
        let mut app = AppState {
            left: crate::app::TabManager::new(std::path::Path::new("/tmp")).unwrap(),
            right: crate::app::TabManager::new(std::path::Path::new("/tmp")).unwrap(),
            active: crate::app::PanelSide::Left,
            file_viewer: FileViewerState::new(false, ""),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(task_tx),
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
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
        };
        app.popups.conflict.is_visible = true;
        app.popups.conflict.task_id = task_id;
        app.popups.conflict.conflict_path = std::path::PathBuf::from("/tmp/fake");
        app.popups.conflict.conflict_type = crate::tasks::ConflictType::FileExists;
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
            let (mut app, mut rx) = app_with_conflict(task_id);
            let still_visible = app.popups.conflict.is_visible;
            assert!(still_visible);
            let _ = handle_conflict_event(key, &mut app).await;
            let recv = rx.try_recv().unwrap();
            assert_eq!(recv, expected_decision);
            assert!(!app.popups.conflict.is_visible);
        }
    }

    #[tokio::test]
    async fn ignores_irrelevant_keys() {
        let (mut app, mut rx) = app_with_conflict(42);
        let _ = handle_conflict_event(KeyCode::Char('z'), &mut app).await;
        assert!(
            app.popups.conflict.is_visible,
            "Unmapped key should not reset popup"
        );
        assert!(
            rx.try_recv().is_err(),
            "No decision should be sent for unmapped key"
        );
    }
}
