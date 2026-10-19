//! Common utilities for popup handlers

use crate::app::AppState;
use termina::event::KeyCode;

/// Whether the active filesystem accepts the write an operation is about to
/// offer, declining and reporting it on `app` when it does not.
///
/// Consults [`FileSystemProvider::capabilities`] and returns
/// `true` when the operation may proceed.
///
/// [`FileSystemProvider::capabilities`]: crate::fs::provider::FileSystemProvider::capabilities
pub fn active_fs_accepts_writes(app: &mut AppState) -> bool {
    if app.active_tab().provider.capabilities().writable {
        return true;
    }
    app.active_tab_mut().error = Some("This filesystem is read-only".to_string());
    false
}

/// Result of a popup choice
pub enum ChoiceResult {
    Confirmed,
    Cancelled,
    None,
}

/// Cycle button focus with Left/Right/Tab/BackTab
/// Returns true if the key was consumed (focus moved)
pub fn handle_button_nav(code: KeyCode, focused: &mut usize, len: usize) -> bool {
    match code {
        KeyCode::Left | KeyCode::BackTab => {
            *focused = if *focused == 0 { len - 1 } else { *focused - 1 };
            true
        }
        KeyCode::Right | KeyCode::Tab => {
            *focused = (*focused + 1) % len;
            true
        }
        _ => false,
    }
}

/// Confirmation handling with Tab/Left/Right selection support for Yes/No buttons
/// - Y/Enter (when Yes is selected): Confirmed
/// - N/Esc/Enter (when No is selected): Cancelled
#[must_use]
pub fn get_choice_with_selection(code: KeyCode, selected_no: &mut bool) -> ChoiceResult {
    match code {
        KeyCode::Char('y' | 'Y') => ChoiceResult::Confirmed,
        KeyCode::Char('n' | 'N') | KeyCode::Escape => ChoiceResult::Cancelled,
        KeyCode::Enter => {
            if *selected_no {
                ChoiceResult::Cancelled
            } else {
                ChoiceResult::Confirmed
            }
        }
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
            *selected_no = !*selected_no;
            ChoiceResult::None
        }
        _ => ChoiceResult::None,
    }
}
