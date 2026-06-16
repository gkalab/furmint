//! Common utilities for popup handlers

use crossterm::event::KeyCode;

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
        KeyCode::Esc | KeyCode::Char('n' | 'N') => ChoiceResult::Cancelled,
        _ => ChoiceResult::None,
    }
}

/// Confirmation handling with Tab/Left/Right selection support for Yes/No buttons
/// - Y/Enter (when Yes is selected): Confirmed
/// - N/Esc/Enter (when No is selected): Cancelled
#[must_use]
pub fn get_choice_with_selection(code: KeyCode, selected_no: &mut bool) -> ChoiceResult {
    match code {
        KeyCode::Char('y' | 'Y') => ChoiceResult::Confirmed,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => ChoiceResult::Cancelled,
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
