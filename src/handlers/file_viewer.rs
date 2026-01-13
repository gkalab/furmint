//! File viewer event handler

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) fn handle_file_viewer_event(code: KeyCode, app: &mut AppState) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::state::FileViewerState;
    use crossterm::event::KeyCode;

    fn test_app(file_lines: usize) -> AppState {
        let mut app = AppState {
            left: crate::app::TabManager {
                tabs: vec![],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::ClipboardBackend::new()),
        };
        // Populate content lines
        app.file_viewer.content = vec!["line".to_string(); file_lines];
        app.file_viewer.scroll_offset = 5;
        app.file_viewer.horizontal_scroll_offset = 15;
        app
    }

    #[test]
    fn test_tab_blur_focus() {
        let mut app = test_app(10);
        app.file_viewer.focused = true;
        handle_file_viewer_event(KeyCode::Tab, &mut app);
        assert!(!app.file_viewer.focused);
    }

    #[test]
    fn test_up_scroll_decrease() {
        let mut app = test_app(10);
        app.file_viewer.scroll_offset = 5;
        handle_file_viewer_event(KeyCode::Up, &mut app);
        assert_eq!(app.file_viewer.scroll_offset, 4);
        handle_file_viewer_event(KeyCode::Up, &mut app);
        assert_eq!(app.file_viewer.scroll_offset, 3);
    }

    #[test]
    fn test_down_scroll_increase() {
        let mut app = test_app(10);
        handle_file_viewer_event(KeyCode::Down, &mut app);
        assert_eq!(app.file_viewer.scroll_offset, 6);
    }

    #[test]
    fn test_left_horizontal_scroll() {
        let mut app = test_app(10);
        handle_file_viewer_event(KeyCode::Left, &mut app);
        assert_eq!(app.file_viewer.horizontal_scroll_offset, 5);
    }

    #[test]
    fn test_right_horizontal_scroll_increase() {
        let mut app = test_app(10);
        handle_file_viewer_event(KeyCode::Right, &mut app);
        assert_eq!(app.file_viewer.horizontal_scroll_offset, 25);
    }

    #[test]
    fn test_pageup_scroll_large_jump() {
        let mut app = test_app(30);
        app.file_viewer.scroll_offset = 25;
        handle_file_viewer_event(KeyCode::PageUp, &mut app);
        assert_eq!(app.file_viewer.scroll_offset, 5);
    }

    #[test]
    fn test_pagedown_scroll_large_jump() {
        let mut app = test_app(30);
        app.file_viewer.scroll_offset = 5;
        handle_file_viewer_event(KeyCode::PageDown, &mut app);
        // Should jump to 25 or max_scroll
        assert!(app.file_viewer.scroll_offset > 5);
    }

    #[test]
    fn test_home_and_end_keys() {
        let mut app = test_app(40);
        handle_file_viewer_event(KeyCode::Home, &mut app);
        assert_eq!(app.file_viewer.scroll_offset, 0);
        handle_file_viewer_event(KeyCode::End, &mut app);
        assert_eq!(
            app.file_viewer.scroll_offset,
            app.file_viewer.content.len() - 1
        );
    }
}
