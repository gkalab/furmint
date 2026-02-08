use crate::app::{AppState, PanelSide};
use crate::config::KeyboardConfig;
use crate::handlers::file_viewer::handle_file_viewer_event;
use crate::handlers::navigation::{reset_expired_search, update_viewer_content};
use crate::handlers::popup_conflict::handle_conflict_event;
use crate::handlers::popup_copy_move::handle_copy_move_event;
use crate::handlers::popup_create::{handle_create_directory_event, handle_create_file_event};
use crate::handlers::popup_delete::handle_delete_event;
use crate::handlers::popup_error::handle_error_event;
use crate::handlers::popup_fuzzy::handle_fuzzy_search_event;
use crate::handlers::popup_misc::{
    handle_quit_popup_event, handle_task_event, handle_task_manager_event,
};
use crate::handlers::popup_rename::handle_rename_event;
use crate::handlers::popup_ssh::{handle_ssh_connection_event, handle_ssh_password_event};
use crate::handlers::terminal::handle_toggle_console;
use crate::theme::ThemePalette;
use crate::ui::{draw_panel, draw_panel_status};
use tokio::sync::mpsc::UnboundedReceiver;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;

pub fn spawn_input_polling(
    input_tx: tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if input_tx.is_closed() {
                break;
            }
            match tokio::task::spawn_blocking(|| event::poll(std::time::Duration::from_millis(100)))
                .await
            {
                Ok(Ok(true)) => match event::read() {
                    Ok(ev) => {
                        if input_tx.send(ev).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(Ok(false)) => {}
                Ok(Err(_)) | Err(_) => break,
            }
        }
    })
}

pub async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut AppState,
    palette: &ThemePalette,
    keyboard: KeyboardConfig,
    watcher_rx: &mut UnboundedReceiver<crate::fs::watcher::WatcherEvent>,
    task_rx: &mut UnboundedReceiver<crate::tasks::TaskEvent>,
    image_load_rx: &mut UnboundedReceiver<crate::state::ImageLoadResult>,
) -> anyhow::Result<()> {
    // Create channel for terminal events
    let (input_tx, mut input_rx) = tokio::sync::mpsc::unbounded_channel();

    // Start input polling and store handle
    app.input_polling_handle = Some(spawn_input_polling(input_tx.clone()));

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

    let mut should_exit = false;

    // Initial draw
    draw_ui(terminal, app, palette, &keyboard)?;

    while !should_exit {
        // Explicit redraw if requested (e.g. after editor or console toggle)
        if app.needs_redraw {
            terminal.clear()?;
            draw_ui(terminal, app, palette, &keyboard)?;
            app.needs_redraw = false;
        }
        tokio::select! {
                            // Handle watcher events
                            Some(event) = watcher_rx.recv() => {
                                handle_watcher_event(event, app);
                                app.sync_watcher();
                                draw_ui(terminal, app, palette, &keyboard)?;
                            }
                            // Handle input events
                            Some(event) = input_rx.recv() => {
                let mut exit = handle_event(event, app, &keyboard, input_tx.clone()).await;
                // Drain any other immediately available events to prevent buffering
                // This allows skipping frames if input is faster than rendering
        while !exit {
            match input_rx.try_recv() {
                Ok(ev) => {
                    if handle_event(ev, app, &keyboard, input_tx.clone()).await {
                        exit = true;
                    }
                }
                Err(_) => break,
            }
        }

                                if exit {
                                    should_exit = true;
                                } else {
                                    if app.needs_redraw {
                                        terminal.clear()?;
                                        app.needs_redraw = false;
                                    }
                                    draw_ui(terminal, app, palette, &keyboard)?;
                                }

                                // Sync watcher if navigation happened
                                app.sync_watcher();
                            }
                            // Handle task events
                            Some(event) = task_rx.recv() => {
                                handle_task_event(event, app);
                                draw_ui(terminal, app, palette, &keyboard)?;
                            }
                            _ = interval.tick() => {
                                app.task_manager.cleanup_tasks();
                                // Reset search if timeout has expired
                                reset_expired_search(app);
                                // Poll watchers
                                if let Some(w) = &mut app.watcher {
                                    let _ = w.poll();
                                }
                                if let Some(w) = &mut app.remote_watcher {
                                    let _ = w.poll();
                                }
                                draw_ui(terminal, app, palette, &keyboard)?;
                            }
                            // Handle image resize requests immediately and off-thread
                            Some(request) = async {
                                if let Some(rx) = &mut app.file_viewer.resize_rx {
                                    rx.recv().await
                                } else {
                                    std::future::pending().await
                                }
                            } => {
                                let encoded = tokio::task::spawn_blocking(move || -> Result<ratatui_image::thread::ResizeResponse, _> {
                                    request.resize_encode()
                                }).await.ok().and_then(|r| r.ok());

                                if let (Some(encoded), Some(protocol)) = (encoded, &mut app.file_viewer.protocol) {
                                    let _ = protocol.update_resized_protocol(encoded);
                                    draw_ui(terminal, app, palette, &keyboard)?;
                                }
                            }
                            // Handle image load results
                            Some(load_result) = image_load_rx.recv() => {
                                app.file_viewer.handle_load_result(load_result);
                                draw_ui(terminal, app, palette, &keyboard)?;
                            }
                            else => break,
                        }
    }
    terminal.clear()?;
    Ok(())
}

fn handle_watcher_event(event: crate::fs::watcher::WatcherEvent, app: &mut AppState) {
    match event {
        crate::fs::watcher::WatcherEvent::FileSystemChange(paths) => {
            let handle_tab = |tab: &mut crate::app::Tab| {
                // Watcher only supports local filesystem
                if !tab.provider.is_local() {
                    return;
                }

                // Check info about current directory
                let current_exists = tab.current_dir.exists();

                if !current_exists {
                    // Directory removed, try to go up
                    let _ = tab.go_up();
                }

                // Check if we need to reload
                let needs_reload = paths
                    .iter()
                    .any(|p| p == &tab.current_dir || p.parent() == Some(&tab.current_dir));

                if needs_reload && let Ok(entries) = tab.provider.list_dir(&tab.current_dir) {
                    tab.reload_preserving_state(entries);
                }
            };

            for tab in &mut app.left.tabs {
                handle_tab(tab);
            }
            for tab in &mut app.right.tabs {
                handle_tab(tab);
            }
        }
        crate::fs::watcher::WatcherEvent::RemoteReloadRequested => {
            app.reload_remote();
        }
        crate::fs::watcher::WatcherEvent::Error(_) => {}
    }
}

fn draw_ui(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut AppState,
    palette: &ThemePalette,
    keyboard: &KeyboardConfig,
) -> anyhow::Result<()> {
    terminal.draw(|f| {
        let size = f.area();
        // Fill the entire terminal with the theme background color
        let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
        let bg = ratatui::widgets::Paragraph::new("").style(Style::default().bg(bg_color));
        f.render_widget(bg, size);

        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // panels
                Constraint::Length(1), // status lines
            ])
            .split(size);
        let panel_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(vertical_chunks[0]);
        let status_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(vertical_chunks[1]);

        let show_tabs = app.left.tabs.len() > 1 || app.right.tabs.len() > 1;

        let left_panel_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                if show_tabs {
                    Constraint::Length(1)
                } else {
                    Constraint::Length(0)
                },
                Constraint::Min(1),
            ])
            .split(panel_chunks[0]);

        let right_panel_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                if show_tabs {
                    Constraint::Length(1)
                } else {
                    Constraint::Length(0)
                },
                Constraint::Min(1),
            ])
            .split(panel_chunks[1]);

        if app.file_viewer.is_visible && app.active == PanelSide::Right {
            crate::ui::draw_file_viewer(
                f,
                &mut app.file_viewer,
                panel_chunks[0],
                palette,
                app.global.borders.unwrap_or(false),
            );
        } else {
            let is_active = app.active == PanelSide::Left && !app.file_viewer.focused;
            if show_tabs {
                crate::ui::draw_tab_bar(
                    f,
                    &app.left,
                    left_panel_layout[0],
                    palette,
                    is_active,
                    app.global.borders.unwrap_or(false),
                    app.global.icons.unwrap_or(false),
                );
            }
            draw_panel(
                f,
                app.left.active_tab_mut(),
                is_active,
                left_panel_layout[1],
                palette,
                app.global.borders.unwrap_or(false),
                app.global.icons.unwrap_or(false),
            );
        }

        if app.file_viewer.is_visible && app.active == PanelSide::Left {
            crate::ui::draw_file_viewer(
                f,
                &mut app.file_viewer,
                panel_chunks[1],
                palette,
                app.global.borders.unwrap_or(false),
            );
        } else {
            let is_active = app.active == PanelSide::Right && !app.file_viewer.focused;
            if show_tabs {
                crate::ui::draw_tab_bar(
                    f,
                    &app.right,
                    right_panel_layout[0],
                    palette,
                    is_active,
                    app.global.borders.unwrap_or(false),
                    app.global.icons.unwrap_or(false),
                );
            }
            draw_panel(
                f,
                app.right.active_tab_mut(),
                is_active,
                right_panel_layout[1],
                palette,
                app.global.borders.unwrap_or(false),
                app.global.icons.unwrap_or(false),
            );
        }

        draw_panel_status(
            f,
            app.left.active_tab(),
            status_chunks[0],
            &crate::ui::panel::PanelStatusContext {
                palette,
                active: app.active == PanelSide::Left,
                borders: app.global.borders.unwrap_or(false),
                task_manager: &app.task_manager,
                side: PanelSide::Left,
            },
        );
        draw_panel_status(
            f,
            app.right.active_tab(),
            status_chunks[1],
            &crate::ui::panel::PanelStatusContext {
                palette,
                active: app.active == PanelSide::Right,
                borders: app.global.borders.unwrap_or(false),
                task_manager: &app.task_manager,
                side: PanelSide::Right,
            },
        );

        // Draw fuzzy search popup on top of everything
        crate::ui::fuzzy_search_ui::draw_fuzzy_search_popup(f, &mut app.fuzzy_search, palette);

        // Draw rename popup on top of fuzzy search (though they shouldn't be open at same time)
        crate::ui::rename_ui::draw_rename_popup(f, &app.popups.rename, palette);

        // Draw create directory popup
        crate::ui::create_dir_ui::draw_create_dir_popup(f, &app.popups.create_directory, palette);
        // Draw create file popup
        crate::ui::create_file_ui::draw_create_file_popup(f, &app.popups.create_file, palette);

        // Draw delete popup
        crate::ui::delete_ui::draw_delete_popup(f, &app.popups.delete, palette);

        // Draw copy/move popup
        crate::ui::copy_move_ui::draw_copy_move_popup(f, &app.popups.copy_move, palette);

        // Draw conflict popup
        crate::ui::conflict_ui::draw_conflict_popup(f, &app.popups.conflict, palette);

        // Draw task manager
        crate::ui::task_ui::draw_task_manager(f, &app.task_manager, app.show_task_manager, palette);

        // Draw empty trash popup
        crate::ui::empty_trash_ui::draw_empty_trash_popup(f, &app.popups.empty_trash, palette);

        // Draw quit confirmation popup
        crate::ui::quit_ui::draw_quit_popup(f, &app.popups.quit_confirmation, palette);

        // Draw error popup
        crate::ui::error_ui::draw_error_popup(f, &app.popups.error, palette);

        // Draw help popup
        crate::ui::help_ui::draw_help_popup(f, app, keyboard, palette);

        // Draw drive selection popup
        if app.popups.drive_select.is_visible {
            crate::drive_select_ui::draw_drive_select_popup(f, app, palette);
        }

        // Draw SSH connection popup
        if app.popups.ssh_connection.is_visible {
            crate::ui::ssh_ui::draw_ssh_connection_popup(f, app, palette);
        }

        // Draw SSH password popup
        if app.popups.ssh_password.is_visible {
            crate::ui::ssh_ui::draw_ssh_password_popup(f, app, palette);
        }

        // Draw remote edit confirmation popup
        crate::ui::remote_edit_ui::draw_remote_edit_popup(f, &app.popups.remote_edit, palette);
    })?;
    Ok(())
}

/// Returns true if the event is a quit event (Ctrl-q or Esc)
pub async fn handle_event(
    ev: Event,
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    input_tx: tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> bool {
    match ev {
        Event::Key(KeyEvent {
            kind: crossterm::event::KeyEventKind::Press,
            code,
            modifiers,
            ..
        }) => {
            // Check configurable quit/exit key(s)
            let shortcut = keyevent_to_string(code, modifiers);
            let quit_match = keyboard
                .quit
                .as_ref()
                .is_some_and(|keys| keys.contains(&shortcut));
            if (quit_match && code != KeyCode::Esc)
                || (quit_match
                    && !app.file_viewer.is_visible
                    && !app.fuzzy_search.is_visible
                    && !app.popups.rename.is_visible
                    && !app.popups.create_directory.is_visible
                    && !app.popups.delete.is_visible
                    && !app.popups.copy_move.is_visible
                    && !app.popups.conflict.is_visible
                    && !app.popups.quit_confirmation.is_visible
                    && !app.popups.error.is_visible
                    && !app.popups.help.is_visible
                    && !app.popups.drive_select.is_visible
                    && !app.popups.remote_edit.is_visible
                    && !app.show_task_manager)
            {
                if app.task_manager.has_running_tasks() {
                    app.popups.quit_confirmation.is_visible = true;
                    return false;
                }
                return true;
            }

            // Handle error popup
            if app.popups.error.is_visible {
                if handle_error_event(code, app).await {
                    return true;
                }
                return false;
            }

            // Handle help popup
            if app.popups.help.is_visible {
                crate::ui::help_ui::handle_help_popup_event(code, app);
                return false;
            }

            // Handle empty trash popup
            if app.popups.empty_trash.is_visible {
                if crate::ui::empty_trash_ui::handle_empty_trash_popup_event(code, app) {
                    return false;
                }
                return false;
            }

            // Handle quit confirmation popup
            if app.popups.quit_confirmation.is_visible {
                if handle_quit_popup_event(code, app) {
                    return true; // Quit confirmed
                }
                return false;
            }

            // Handle fuzzy search popup
            if app.fuzzy_search.is_visible {
                return handle_fuzzy_search_event(code, modifiers, app);
            }

            // Handle rename popup
            if app.popups.rename.is_visible {
                return handle_rename_event(code, modifiers, app);
            }

            // Handle create directory popup
            if app.popups.create_directory.is_visible {
                return handle_create_directory_event(code, modifiers, app);
            }

            if app.popups.create_file.is_visible {
                return handle_create_file_event(code, modifiers, app, &input_tx).await;
            }

            // Handle delete popup
            if app.popups.delete.is_visible {
                return handle_delete_event(code, app);
            }

            // Handle copy/move popup
            if app.popups.copy_move.is_visible {
                return handle_copy_move_event(code, modifiers, app);
            }

            // Handle drive selection popup
            if app.popups.drive_select.is_visible {
                return crate::drive_select_ui::handle_drive_select_event(code, app);
            }

            // Handle conflict popup
            if app.popups.conflict.is_visible {
                return handle_conflict_event(code, app).await;
            }

            // Handle SSH connection popup
            if app.popups.ssh_connection.is_visible {
                return handle_ssh_connection_event(app, code, modifiers);
            }

            // Handle SSH password popup
            if app.popups.ssh_password.is_visible {
                return handle_ssh_password_event(app, code, modifiers);
            }

            // Handle remote edit confirmation popup
            if app.popups.remote_edit.is_visible {
                return crate::handlers::editor::handle_remote_edit_event(code, app).await;
            }

            // Handle task manager
            if app.show_task_manager {
                return handle_task_manager_event(code, app);
            }

            if code == KeyCode::F(3) && modifiers == KeyModifiers::NONE {
                if crate::handlers::file_viewer::handle_external_viewer(app).await {
                    return false;
                }
                app.file_viewer.is_visible = !app.file_viewer.is_visible;
                if app.file_viewer.is_visible {
                    update_viewer_content(app);
                } else {
                    // If viewer was focused, switch to the panel it was replacing
                    if app.file_viewer.focused {
                        app.active = match app.active {
                            PanelSide::Left => PanelSide::Right,
                            PanelSide::Right => PanelSide::Left,
                        };
                    }
                    app.file_viewer.focused = false;
                }
                return false;
            }
            // Esc must always close the file viewer if it's open (even if Esc is not mapped globally)
            if app.file_viewer.is_visible && code == KeyCode::Esc {
                app.file_viewer.is_visible = false;
                app.file_viewer.focused = false;
                return false;
            }
            if app.file_viewer.focused {
                handle_file_viewer_event(code, app);
                return false;
            }

            // Handle toggle console
            let toggle_console_match = keyboard
                .toggle_console
                .as_ref()
                .is_some_and(|keys| keys.contains(&shortcut));
            if toggle_console_match {
                if let Err(e) = handle_toggle_console(app, &input_tx).await {
                    let tab_manager = match app.active {
                        PanelSide::Left => &mut app.left,
                        PanelSide::Right => &mut app.right,
                    };
                    tab_manager.active_tab_mut().error =
                        Some(format!("Error toggling console: {e}"));
                }
                return false;
            }

            handle_main_panel_event(code, modifiers, app, keyboard, input_tx.clone()).await;
        }
        Event::Resize(_, _) => {}
        _ => {}
    }
    false
}

async fn handle_main_panel_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    input_tx: tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> bool {
    crate::handlers::input::handle_main_panel_event(code, modifiers, app, keyboard, input_tx).await
}

fn keyevent_to_string(code: KeyCode, modifiers: KeyModifiers) -> String {
    let mut parts: Vec<String> = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    let key = match code {
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => String::new(),
    };
    parts.push(key);
    parts.join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyevent_to_string() {
        assert_eq!(
            keyevent_to_string(KeyCode::F(3), KeyModifiers::CONTROL),
            "Ctrl-F3"
        );
        assert_eq!(
            keyevent_to_string(KeyCode::Char('p'), KeyModifiers::CONTROL),
            "Ctrl-p"
        );
        assert_eq!(
            keyevent_to_string(KeyCode::Left, KeyModifiers::ALT),
            "Alt-Left"
        );
    }

    #[test]
    fn test_handle_watcher_event_filesystem_change() {
        // Setup AppState mock: two tabs, stub current_dir, fake entries
        use crate::fs::watcher::WatcherEvent;
        let mut app = crate::app::AppState {
            left: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![crate::fs::utils::FileEntry {
                        name: "testfile.txt".to_string(),
                        is_dir: false,
                        is_symlink: false,
                        size: Some(12),
                        modified: None,
                        attributes: "".to_string(),
                        selected: false,
                    }],
                    cursor: 0,
                    history: crate::app_state::tabs::TabHistory::new(
                        std::path::PathBuf::from("/mock"),
                        0,
                    ),
                    search: crate::app_state::tabs::IncrementalSearch::default(),
                    sort: crate::app_state::tabs::SortSettings::default(),
                    scroll_offset: 0,
                    error: None,
                    custom_title: None,
                    clipboard_msg: None,
                }],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: crate::app_state::tabs::TabHistory::new(
                        std::path::PathBuf::from("/mock"),
                        0,
                    ),
                    search: crate::app_state::tabs::IncrementalSearch::default(),
                    sort: crate::app_state::tabs::SortSettings::default(),
                    scroll_offset: 0,
                    error: None,
                    custom_title: None,
                    clipboard_msg: None,
                }],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            remote_watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::InMemoryFileClipboard::new()),
        };
        let paths = vec![std::path::PathBuf::from("/mock")];
        let event = WatcherEvent::FileSystemChange(paths);
        super::handle_watcher_event(event, &mut app);
        // Check cursor and error remain valid
        assert!(app.left.active_tab().cursor == 0);
        assert!(app.left.active_tab().error.is_none());
    }

    #[test]
    fn test_handle_watcher_event_preserves_selection() {
        use crate::fs::utils::FileEntry;
        use crate::fs::watcher::WatcherEvent;
        let mut app = crate::app::AppState {
            left: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![FileEntry {
                        name: "testfile.txt".to_string(),
                        is_dir: false,
                        is_symlink: false,
                        size: Some(12),
                        modified: None,
                        attributes: "".to_string(),
                        selected: true, // Initially selected
                    }],
                    cursor: 0,
                    history: crate::app_state::tabs::TabHistory::new(
                        std::path::PathBuf::from("/mock"),
                        0,
                    ),
                    search: crate::app_state::tabs::IncrementalSearch::default(),
                    sort: crate::app_state::tabs::SortSettings::default(),
                    scroll_offset: 0,
                    error: None,
                    custom_title: None,
                    clipboard_msg: None,
                }],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: crate::app_state::tabs::TabHistory::new(
                        std::path::PathBuf::from("/mock"),
                        0,
                    ),
                    search: crate::app_state::tabs::IncrementalSearch::default(),
                    sort: crate::app_state::tabs::SortSettings::default(),
                    scroll_offset: 0,
                    error: None,
                    custom_title: None,
                    clipboard_msg: None,
                }],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            remote_watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::InMemoryFileClipboard::new()),
        };

        // Note: we need to mock list_dir or ensure it returns what we expect.
        // In this test, handle_watcher_event will call list_dir("/mock").
        // Since "/mock" doesn't exist, list_dir will fail, and it won't reload entries.
        // Wait, I need a real directory to test reload.

        let tmp_dir = tempfile::tempdir().unwrap();
        let file_path = tmp_dir.path().join("testfile.txt");
        std::fs::File::create(&file_path).unwrap();

        app.left.active_tab_mut().current_dir = tmp_dir.path().to_path_buf();
        app.left.active_tab_mut().entries = vec![FileEntry {
            name: "testfile.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(0),
            modified: None,
            attributes: "".to_string(),
            selected: true,
        }];

        let paths = vec![tmp_dir.path().to_path_buf()];
        let event = WatcherEvent::FileSystemChange(paths);

        super::handle_watcher_event(event, &mut app);

        // Check if selection is preserved
        assert!(
            app.left
                .active_tab()
                .entries
                .iter()
                .any(|e| e.name == "testfile.txt" && e.selected),
            "Selection should be preserved after watcher reload"
        );
    }

    #[test]
    fn test_handle_watcher_event_error() {
        use crate::fs::watcher::WatcherEvent;
        let mut app = crate::app::AppState {
            left: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: crate::app_state::tabs::TabHistory::new(
                        std::path::PathBuf::from("/mock"),
                        0,
                    ),
                    search: crate::app_state::tabs::IncrementalSearch::default(),
                    sort: crate::app_state::tabs::SortSettings::default(),
                    scroll_offset: 0,
                    error: None,
                    custom_title: None,
                    clipboard_msg: None,
                }],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: crate::app_state::tabs::TabHistory::new(
                        std::path::PathBuf::from("/mock"),
                        0,
                    ),
                    search: crate::app_state::tabs::IncrementalSearch::default(),
                    sort: crate::app_state::tabs::SortSettings::default(),
                    scroll_offset: 0,
                    error: None,
                    custom_title: None,
                    clipboard_msg: None,
                }],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            remote_watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::InMemoryFileClipboard::new()),
        };
        let event = WatcherEvent::Error("test error".to_string());
        super::handle_watcher_event(event, &mut app);
        // Should not panic or change error field
        assert!(app.left.active_tab().error.is_none());
    }

    #[tokio::test]
    async fn test_handle_insert_moves_cursor_down() {
        use crate::fs::utils::FileEntry;
        let mut app = crate::app::AppState {
            left: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![
                        FileEntry {
                            name: "file1.txt".to_string(),
                            is_dir: false,
                            is_symlink: false,
                            size: Some(10),
                            modified: None,
                            attributes: "".to_string(),
                            selected: false,
                        },
                        FileEntry {
                            name: "file2.txt".to_string(),
                            is_dir: false,
                            is_symlink: false,
                            size: Some(10),
                            modified: None,
                            attributes: "".to_string(),
                            selected: false,
                        },
                    ],
                    cursor: 0,
                    history: crate::app_state::tabs::TabHistory::new(
                        std::path::PathBuf::from("/mock"),
                        0,
                    ),
                    search: crate::app_state::tabs::IncrementalSearch::default(),
                    sort: crate::app_state::tabs::SortSettings::default(),
                    scroll_offset: 0,
                    error: None,
                    custom_title: None,
                    clipboard_msg: None,
                }],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: crate::app_state::tabs::TabHistory::new(
                        std::path::PathBuf::from("/mock"),
                        0,
                    ),
                    search: crate::app_state::tabs::IncrementalSearch::default(),
                    sort: crate::app_state::tabs::SortSettings::default(),
                    scroll_offset: 0,
                    error: None,
                    custom_title: None,
                    clipboard_msg: None,
                }],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history: crate::dir_history::DirectoryHistory::new().unwrap(),
            watcher: None,
            remote_watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::InMemoryFileClipboard::new()),
        };

        let keyboard = KeyboardConfig::default();
        let (input_tx, _) = tokio::sync::mpsc::unbounded_channel();

        // Initial state: cursor at 0, file1 not selected
        assert_eq!(app.left.active_tab().cursor, 0);
        assert!(!app.left.active_tab().entries[0].selected);

        handle_main_panel_event(
            KeyCode::Insert,
            KeyModifiers::NONE,
            &mut app,
            &keyboard,
            input_tx,
        )
        .await;

        // After Insert: file1 should be selected, cursor should be at 1
        assert!(app.left.active_tab().entries[0].selected);
        assert_eq!(app.left.active_tab().cursor, 1);
    }
}
