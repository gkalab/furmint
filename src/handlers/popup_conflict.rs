//! Conflict popup event handler

use crate::app::AppState;
use crossterm::event::KeyCode;

pub async fn handle_conflict_event(code: KeyCode, app: &mut AppState) -> bool {
    let task_id = app.popups.conflict.task_id;
    let decision = match code {
        KeyCode::Char('o' | 'O') => Some(crate::tasks::TaskDecision::Overwrite),
        KeyCode::Char('s' | 'S') => Some(crate::tasks::TaskDecision::Skip),
        KeyCode::Char('c' | 'C') | KeyCode::Esc => Some(crate::tasks::TaskDecision::Cancel),
        KeyCode::Char('a' | 'A') => Some(crate::tasks::TaskDecision::OverwriteAll),
        KeyCode::Char('p' | 'P' | 'n' | 'N') => Some(crate::tasks::TaskDecision::SkipAll),
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
