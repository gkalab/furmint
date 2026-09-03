//! Error popup event handler

use crate::app::AppState;
use crate::handlers::popup_utils::handle_button_nav;
use termina::event::KeyCode;

pub(crate) async fn handle_error_event(code: KeyCode, app: &mut AppState) -> bool {
    let task_id = app.popups.error.task_id;

    if handle_button_nav(code, &mut app.popups.error.focused_button, 4) {
        return false;
    }

    let decision = match code {
        KeyCode::Enter => match app.popups.error.focused_button {
            0 => Some(crate::tasks::TaskDecision::Cancel),
            1 => Some(crate::tasks::TaskDecision::Skip),
            2 => Some(crate::tasks::TaskDecision::SkipAll),
            3 => Some(crate::tasks::TaskDecision::Retry),
            _ => None,
        },
        KeyCode::Char('r' | 'R') => Some(crate::tasks::TaskDecision::Retry),
        KeyCode::Char('s' | 'S') => Some(crate::tasks::TaskDecision::Skip),
        KeyCode::Char('a' | 'A') => Some(crate::tasks::TaskDecision::SkipAll),
        KeyCode::Char('c' | 'C') | KeyCode::Escape => Some(crate::tasks::TaskDecision::Cancel),
        _ => None,
    };
    if let Some(d) = decision {
        if let Some(tx) = app.task_decision_txs.get(&task_id) {
            let _ = tx.send(d).await;
        }
        app.popups.reset_popup(crate::app::PopupKind::Error);
    }
    false
}
