//! File viewer event handler

use crate::app::AppState;
use crossterm::event::KeyCode;

pub fn handle_file_viewer_event(code: KeyCode, app: &mut AppState) {
    match code {
        KeyCode::Tab => {
            app.file_viewer.focused = false;
        }
        KeyCode::Up => {
            if app.file_viewer.scroll_offset > 0 {
                app.file_viewer.scroll_offset -= 1;
            }
        }
        KeyCode::Down => {
            if app.file_viewer.scroll_offset + 1 < app.file_viewer.content.len() {
                app.file_viewer.scroll_offset += 1;
            }
        }
        KeyCode::Left => {
            if app.file_viewer.horizontal_scroll_offset >= 10 {
                app.file_viewer.horizontal_scroll_offset -= 10;
            } else {
                app.file_viewer.horizontal_scroll_offset = 0;
            }
        }
        KeyCode::Right => {
            app.file_viewer.horizontal_scroll_offset += 10;
        }
        KeyCode::PageUp => {
            let visible_rows = 20;
            if app.file_viewer.scroll_offset >= visible_rows {
                app.file_viewer.scroll_offset -= visible_rows;
            } else {
                app.file_viewer.scroll_offset = 0;
            }
        }
        KeyCode::PageDown => {
            let visible_rows = 20;
            let max_scroll = app.file_viewer.content.len().saturating_sub(1);
            if app.file_viewer.scroll_offset + visible_rows <= max_scroll {
                app.file_viewer.scroll_offset += visible_rows;
            } else {
                app.file_viewer.scroll_offset = max_scroll;
            }
        }
        KeyCode::Home => {
            app.file_viewer.scroll_offset = 0;
        }
        KeyCode::End => {
            app.file_viewer.scroll_offset = app.file_viewer.content.len().saturating_sub(1);
        }
        _ => {}
    }
}

pub async fn handle_external_viewer(app: &mut AppState) -> bool {
    let viewer_cmd = app.viewer_cfg.command.as_ref().cloned();

    if let Some(cmd_str) = viewer_cmd {
        if let Some(entry) = app.active_tab().current_entry().cloned()
            && !entry.is_dir
        {
            let file_path = app.active_tab().current_dir.join(&entry.name);
            let in_terminal = app.viewer_cfg.in_terminal.unwrap_or(true);

            if let Err(e) = crate::handlers::external::launch_external_program(
                app,
                &cmd_str,
                file_path,
                in_terminal,
                "viewer",
            )
            .await
            {
                app.active_tab_mut().error = Some(e);
            }
        }
        return true;
    }
    false
}
