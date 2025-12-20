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
