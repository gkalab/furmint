//! File viewer event handler

use crate::app::AppState;
use termina::event::{KeyCode, Modifiers};

pub fn handle_file_viewer_event(code: KeyCode, modifiers: Modifiers, app: &mut AppState) {
    if handle_viewer_copy(code, modifiers, app) {
        return;
    }

    match code {
        KeyCode::Tab => app.file_viewer.focused = false,
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Home
        | KeyCode::End => {
            handle_viewer_navigation(code, app);
        }
        _ => handle_viewer_shortcuts(code, modifiers, app),
    }
}

fn handle_viewer_copy(code: KeyCode, modifiers: Modifiers, app: &mut AppState) -> bool {
    if code == KeyCode::Char('c') && modifiers == Modifiers::CONTROL {
        if let Some(text) = app.file_viewer.get_selected_text() {
            if let Err(e) = crate::handlers::clipboard_utils::set_clipboard_text(&text) {
                app.viewer_tab_mut().error = Some(format!("Failed to set clipboard: {e}"));
            } else {
                app.viewer_tab_mut().status_msg = Some((
                    "Text copied to clipboard".to_string(),
                    std::time::Instant::now(),
                ));
            }
        }
        return true;
    }
    false
}

fn handle_viewer_navigation(code: KeyCode, app: &mut AppState) {
    if app.file_viewer.is_image_zoomed() {
        handle_image_pan(code, app);
        return;
    }
    match code {
        KeyCode::Up => {
            app.file_viewer.scroll_offset = app.file_viewer.scroll_offset.saturating_sub(1);
        }
        KeyCode::Down => {
            app.file_viewer.scroll_offset =
                (app.file_viewer.scroll_offset + 1).min(app.file_viewer.max_scroll_offset());
        }
        KeyCode::Left => {
            app.file_viewer.horizontal_scroll_offset =
                app.file_viewer.horizontal_scroll_offset.saturating_sub(10);
        }
        KeyCode::Right => {
            app.file_viewer.horizontal_scroll_offset += 10;
        }
        KeyCode::PageUp => {
            app.file_viewer.scroll_offset = app.file_viewer.scroll_offset.saturating_sub(20);
        }
        KeyCode::PageDown => {
            app.file_viewer.scroll_offset =
                (app.file_viewer.scroll_offset + 20).min(app.file_viewer.max_scroll_offset());
        }
        KeyCode::Home => app.file_viewer.scroll_offset = 0,
        KeyCode::End => app.file_viewer.scroll_offset = app.file_viewer.max_scroll_offset(),
        _ => {}
    }
}

/// Pan the zoomed-in image with the arrow/page/home/end keys.
fn handle_image_pan(code: KeyCode, app: &mut AppState) {
    let viewer = &mut app.file_viewer;
    let step = i64::from((viewer.render_area.width / 10).max(1));
    let (dx, dy): (i64, i64) = match code {
        KeyCode::Up => (0, -step),
        KeyCode::Down => (0, step),
        KeyCode::Left => (-step, 0),
        KeyCode::Right => (step, 0),
        KeyCode::PageUp => (0, -(step * 5)),
        KeyCode::PageDown => (0, step * 5),
        KeyCode::Home => (
            -i64::from(viewer.render_area.width),
            -i64::from(viewer.render_area.height),
        ),
        KeyCode::End => (
            i64::from(viewer.render_area.width),
            i64::from(viewer.render_area.height),
        ),
        _ => return,
    };
    viewer.pan_image(dx, dy);
}

fn handle_viewer_shortcuts(code: KeyCode, modifiers: Modifiers, app: &mut AppState) {
    let shortcut = crate::handlers::input_utils::keyevent_to_string(code, modifiers);

    // Image zoom (+ / = zoom in, - zoom out); only meaningful when an image is displayed.
    if let KeyCode::Char(c) = code
        && (c == '+' || c == '=')
        && app.file_viewer.image_zoom.image.is_some()
    {
        app.file_viewer.zoom_image_in();
        return;
    }
    if let KeyCode::Char('-') = code
        && app.file_viewer.image_zoom.image.is_some()
    {
        app.file_viewer.zoom_image_out();
        return;
    }

    if app
        .keyboard
        .viewer_search
        .as_ref()
        .is_some_and(|keys| keys.contains(&shortcut))
    {
        app.popups.viewer_search.is_visible = true;
        app.popups.viewer_search.query = app.file_viewer.search_query.clone();
        app.popups.viewer_search.cursor_position = app.popups.viewer_search.query.chars().count();
        app.popups.viewer_search.error = None;
    } else if app
        .keyboard
        .help
        .as_ref()
        .is_some_and(|keys| keys.contains(&shortcut))
    {
        app.popups.help.is_visible = true;
    } else if app
        .keyboard
        .viewer_search_next
        .as_ref()
        .is_some_and(|keys| {
            keys.contains(&shortcut)
                || (if let KeyCode::Char(c) = code {
                    keys.contains(&c.to_string())
                } else {
                    false
                })
        })
    {
        if !app.file_viewer.search_next() {
            app.viewer_tab_mut().status_msg = Some((
                "No more matches found".to_string(),
                std::time::Instant::now(),
            ));
        }
    } else if app
        .keyboard
        .viewer_search_prev
        .as_ref()
        .is_some_and(|keys| {
            keys.contains(&shortcut)
                || (if let KeyCode::Char(c) = code {
                    keys.contains(&c.to_string())
                } else {
                    false
                })
        })
        && !app.file_viewer.search_prev()
    {
        app.viewer_tab_mut().status_msg = Some((
            "No previous matches found".to_string(),
            std::time::Instant::now(),
        ));
    }
}

pub fn handle_viewer_search_event(
    code: KeyCode,
    _modifiers: Modifiers,
    app: &mut AppState,
) -> bool {
    match code {
        KeyCode::Escape => {
            app.popups.viewer_search.is_visible = false;
        }
        KeyCode::Enter => {
            let query = app.popups.viewer_search.query.clone();
            if !app.file_viewer.search(&query) {
                app.viewer_tab_mut().status_msg = Some((
                    format!("Search string not found: {query}"),
                    std::time::Instant::now(),
                ));
            }
            app.popups.viewer_search.is_visible = false;
        }
        KeyCode::Char(c) => {
            app.popups
                .viewer_search
                .query
                .insert(app.popups.viewer_search.cursor_position, c);
            app.popups.viewer_search.cursor_position += 1;
        }
        KeyCode::Backspace if app.popups.viewer_search.cursor_position > 0 => {
            app.popups.viewer_search.cursor_position -= 1;
            app.popups
                .viewer_search
                .query
                .remove(app.popups.viewer_search.cursor_position);
        }
        KeyCode::Delete
            if app.popups.viewer_search.cursor_position < app.popups.viewer_search.query.len() =>
        {
            app.popups
                .viewer_search
                .query
                .remove(app.popups.viewer_search.cursor_position);
        }
        KeyCode::Left if app.popups.viewer_search.cursor_position > 0 => {
            app.popups.viewer_search.cursor_position -= 1;
        }
        KeyCode::Right
            if app.popups.viewer_search.cursor_position < app.popups.viewer_search.query.len() =>
        {
            app.popups.viewer_search.cursor_position += 1;
        }
        KeyCode::Home => {
            app.popups.viewer_search.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.viewer_search.cursor_position = app.popups.viewer_search.query.len();
        }
        _ => {}
    }
    false
}

pub fn handle_external_viewer(app: &mut AppState) -> bool {
    let viewer_cmd = app.viewer_cfg.command.clone();

    if let Some(cmd_str) = viewer_cmd {
        if let Some(entry) = app.active_tab().current_entry().cloned()
            && !entry.is_dir
        {
            let file_path = app.active_tab().current_dir.join(&entry.name);
            let in_terminal = app.viewer_cfg.in_terminal.unwrap_or(true);

            if let Err(e) = crate::handlers::external::launch_external_program(
                app,
                &cmd_str,
                &file_path,
                in_terminal,
                "viewer",
            ) {
                app.active_tab_mut().error = Some(e);
            }
        }
        return true;
    }
    false
}
