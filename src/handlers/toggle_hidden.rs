//! Global show/hide of hidden files and directories.

use crate::app::AppState;

/// Flips the hidden-file setting for every tab of both panels.
///
/// The setting is global rather than per tab, so this also reconciles any tab
/// that was listed before the setting last changed. Any file filter stays active
/// and is simply re-evaluated against the new setting.
///
/// A viewer showing a directory listing is refreshed so its rows follow the new
/// setting immediately.
pub async fn handle_toggle_hidden(app: &mut AppState) {
    app.set_show_hidden(!app.show_hidden);

    let msg = if app.show_hidden {
        "Hidden files shown"
    } else {
        "Hidden files hidden"
    };
    app.active_tab_mut().status_msg = Some((msg.to_string(), std::time::Instant::now()));

    // Persist immediately rather than only on exit, so the choice survives a
    // crash and is already on disk if the session is resumed from another shell.
    if let Err(e) = app.save_state() {
        app.active_tab_mut().error = Some(format!("Could not save state: {e}"));
    }

    if app.file_viewer.is_visible && app.file_viewer.text.directory_rows.is_some() {
        crate::handlers::navigation::update_viewer_content(app).await;
    }
}
