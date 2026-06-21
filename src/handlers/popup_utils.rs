//! Common utilities for popup handlers

use termina::event::KeyCode;

/// Result of a popup choice
pub enum ChoiceResult {
    Confirmed,
    Cancelled,
    None,
}

/// Standard confirmation handling for popups (Y/N/Enter/Esc)
#[must_use]
pub fn get_choice(code: KeyCode) -> ChoiceResult {
    match code {
        KeyCode::Enter | KeyCode::Char('y' | 'Y') => ChoiceResult::Confirmed,
        KeyCode::Escape | KeyCode::Char('n' | 'N') => ChoiceResult::Cancelled,
        _ => ChoiceResult::None,
    }
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
