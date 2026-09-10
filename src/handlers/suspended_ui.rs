//! Suspends the interactive TUI (input polling, mouse capture, file watchers)
//! while an external program such as a shell or editor runs, then restores it.

use crate::app::AppState;
use crate::handlers::terminal::{disable_mouse_capture, enable_mouse_capture};
use std::io::Write;

/// Raw sequence: show cursor, clear the screen, move to the home position.
pub const CLEAR_TERMINAL_SEQ: &str = "\x1b[?25h\x1b[2J\x1b[H";

/// Clears the terminal screen and resets the cursor to the home position.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn clear_terminal_screen() -> std::io::Result<()> {
    write!(std::io::stdout(), "{CLEAR_TERMINAL_SEQ}")?;
    std::io::stdout().flush()
}

/// Suspended TUI state, restored by [`SuspendedUi::restore`] or partially by
/// [`Drop for SuspendedUi`].
pub struct SuspendedUi {
    mouse_was_enabled: bool,
}

impl SuspendedUi {
    /// Suspends the interactive parts of `app` before spawning an external
    /// program: aborts the input polling task, clears the screen, disables
    /// mouse capture, and unregisters all watched paths.
    pub fn enter(app: &mut AppState) -> Self {
        if let Some(handle) = app.input_polling_handle.take() {
            handle.abort();
        }
        let _ = clear_terminal_screen();
        let mouse_was_enabled = app.global.mouse.unwrap_or(true);
        if mouse_was_enabled {
            let _ = disable_mouse_capture();
        }
        Self::unwatch_all(app);
        Self { mouse_was_enabled }
    }

    /// Restores mouse capture and re-syncs the file watchers after the
    /// external program exits. Safe to call multiple times.
    pub fn restore(&mut self, app: &mut AppState) {
        app.sync_watcher();
        self.reenable_mouse();
    }

    fn unwatch_all(app: &mut AppState) {
        if let Some(watcher) = &mut app.watcher {
            for path in watcher.watched_paths() {
                let _ = watcher.unwatch(&path);
            }
        }
    }

    fn reenable_mouse(&mut self) {
        if self.mouse_was_enabled {
            let _ = enable_mouse_capture();
            self.mouse_was_enabled = false;
        }
    }
}

impl Drop for SuspendedUi {
    /// Re-enables mouse capture if `restore` was never called. Watcher state
    /// needs `&mut AppState` and cannot be restored here, so callers must use
    /// [`SuspendedUi::restore`] for a full restore.
    fn drop(&mut self) {
        self.reenable_mouse();
    }
}
