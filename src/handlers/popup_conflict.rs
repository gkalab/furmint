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
