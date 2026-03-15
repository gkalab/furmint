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
