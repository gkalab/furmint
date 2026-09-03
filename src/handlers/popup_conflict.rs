//! Conflict popup event handler

use crate::app::AppState;
use crate::handlers::popup_utils::handle_button_nav;
use termina::event::KeyCode;

pub async fn handle_conflict_event(code: KeyCode, app: &mut AppState) -> bool {
    let task_id = app.popups.conflict.task_id;

    if handle_button_nav(code, &mut app.popups.conflict.focused_button, 5) {
        return false;
    }

    let decision = match code {
        KeyCode::Enter => match app.popups.conflict.focused_button {
            0 => Some(crate::tasks::TaskDecision::Cancel),
            1 => Some(crate::tasks::TaskDecision::Skip),
            2 => Some(crate::tasks::TaskDecision::Overwrite),
            3 => Some(crate::tasks::TaskDecision::SkipAll),
            4 => Some(crate::tasks::TaskDecision::OverwriteAll),
            _ => None,
        },
        KeyCode::Char('o' | 'O') => Some(crate::tasks::TaskDecision::Overwrite),
        KeyCode::Char('s' | 'S') => Some(crate::tasks::TaskDecision::Skip),
        KeyCode::Char('c' | 'C') | KeyCode::Escape => Some(crate::tasks::TaskDecision::Cancel),
        KeyCode::Char('a' | 'A') => Some(crate::tasks::TaskDecision::OverwriteAll),
        KeyCode::Char('p' | 'P' | 'n' | 'N') => Some(crate::tasks::TaskDecision::SkipAll),
        KeyCode::Char('m' | 'M') => Some(crate::tasks::TaskDecision::Merge),
        _ => None,
    };

    if let Some(d) = decision {
        if let Some(tx) = app.tasks.task_decision_txs.get(&task_id) {
            let _ = tx.send(d).await;
        }
        app.popups.reset_popup(crate::app::PopupKind::Conflict);
    }
    false
}
