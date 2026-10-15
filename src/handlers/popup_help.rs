//! Help popup event handler.
//!
//! The popup's rendering lives in [`crate::ui::help_ui`]; its key handling lives
//! here with the other popup handlers.

use crate::app::AppState;
use termina::event::KeyCode;

pub fn handle_help_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    // The draw pass clamps `scroll_offset` to the real maximum, so this handler
    // can step past the end (e.g. `End` sets `usize::MAX`) and let draw settle
    // it on the next frame.
    let page_step = 10;

    match code {
        KeyCode::Escape => {
            app.popups.reset_popup(crate::app::PopupKind::Help);
        }
        KeyCode::Up => {
            app.popups.help.scroll_offset = app.popups.help.scroll_offset.saturating_sub(1);
        }
        KeyCode::Down => {
            app.popups.help.scroll_offset = app.popups.help.scroll_offset.saturating_add(1);
        }
        KeyCode::PageUp => {
            app.popups.help.scroll_offset = app.popups.help.scroll_offset.saturating_sub(page_step);
        }
        KeyCode::PageDown => {
            app.popups.help.scroll_offset = app.popups.help.scroll_offset.saturating_add(page_step);
        }
        KeyCode::Home => {
            app.popups.help.scroll_offset = 0;
        }
        KeyCode::End => {
            app.popups.help.scroll_offset = usize::MAX; // Draw will clamp
        }
        _ => return false,
    }
    true
}
