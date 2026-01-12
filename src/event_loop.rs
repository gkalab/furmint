// Clippy allows - these will be addressed in Phase 3 modularization
#![allow(clippy::too_many_lines)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::match_same_arms)]

use crate::app::{AppState, PanelSide};
use crate::config::KeyboardConfig;
use crate::handlers::editor::handle_edit;
use crate::handlers::file_viewer::handle_file_viewer_event;
use crate::handlers::navigation::{
    handle_directory_up, handle_down, handle_down_search, handle_end, handle_enter_directory,
    handle_history_next, handle_history_previous, handle_home, handle_open_item, handle_page_down,
    handle_page_up, handle_sort, handle_tab, handle_toggle_selection, handle_type_char, handle_up,
    handle_up_search, reset_expired_search, reset_search, update_viewer_content,
};
use crate::handlers::popup_conflict::handle_conflict_event;
use crate::handlers::popup_copy_move::{
    handle_copy_move_event, handle_init_copy, handle_init_move,
};
use crate::handlers::popup_create::{
    handle_create_directory_event, handle_create_file_event, handle_init_create_directory,
    handle_init_create_file,
};
use crate::handlers::popup_delete::{handle_delete_event, handle_init_delete};
use crate::handlers::popup_error::handle_error_event;
use crate::handlers::popup_fuzzy::handle_fuzzy_search_event;
use crate::handlers::popup_misc::{
    handle_quit_popup_event, handle_task_event, handle_task_manager_event,
};
use crate::handlers::popup_rename::{handle_init_rename, handle_rename_event};
use crate::handlers::popup_ssh::{handle_ssh_connection_event, handle_ssh_password_event};
use crate::handlers::tabs::{handle_close_tab, handle_new_tab, handle_next_tab, handle_prev_tab};
use crate::handlers::terminal::{handle_open_terminal, handle_toggle_console};
use crate::theme::ThemePalette;
use crate::ui::{draw_panel, draw_panel_status};

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
    watcher_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::watcher::WatcherEvent>,
    task_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::tasks::TaskEvent>,
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
                                draw_ui(terminal, app, palette, &keyboard)?;
                            }
                            else => break,
                        }
    }
    terminal.clear()?;
    Ok(())
}

fn handle_watcher_event(event: crate::watcher::WatcherEvent, app: &mut AppState) {
    match event {
        crate::watcher::WatcherEvent::FileSystemChange(paths) => {
            let handle_tab = |tab: &mut crate::app::Tab| {
                // Watcher only supports local filesystem
                if !tab.provider.is_local() {
                    return;
                }

                // Check info about current directory
                let current_exists = tab.current_dir.exists();

                if !current_exists {
                    // Directory removed, try to go up
                    // We don't check for errors here, just try
                    let _ = tab.go_up();
                }

                // Check if we need to reload
                // 1. If we just went up, we likely loaded new content, but check logic below
                // 2. If current dir is in paths (it changed itself)
                // 3. If any path's parent is current dir (content of dir changed)
                let needs_reload = paths
                    .iter()
                    .any(|p| p == &tab.current_dir || p.parent() == Some(&tab.current_dir));

                if needs_reload {
                    // Refresh entries
                    // Note: if we just went up, entries are fresh, but reloading again is safe
                    // We try to preserve cursor if possible by matching name?
                    // But standard behavior is reload.
                    // If we want to preserve cursor position on simple content change (like file size update):
                    // Tab::navigate_to calls list_dir which resets cursor unless in history.
                    // But here we are staying in same dir usually.

                    let old_cursor_name = tab.current_entry().map(|e| e.name.clone());
                    let selected_names: std::collections::HashSet<String> = tab
                        .entries
                        .iter()
                        .filter(|e| e.selected)
                        .map(|e| e.name.clone())
                        .collect();

                    if let Ok(entries) = tab.provider.list_dir(&tab.current_dir) {
                        tab.entries = entries;

                        // Restore selection
                        for entry in &mut tab.entries {
                            if selected_names.contains(&entry.name) {
                                entry.selected = true;
                            }
                        }

                        tab.sort_entries();

                        // Try to restore cursor to same file
                        if let Some(name) = old_cursor_name {
                            if let Some(idx) = tab.entries.iter().position(|e| e.name == name) {
                                tab.cursor = idx;
                            } else {
                                // File gone, keep cursor within bounds
                                if tab.cursor >= tab.entries.len() {
                                    tab.cursor = tab.entries.len().saturating_sub(1);
                                }
                            }
                        }
                    }
                }
            };

            for tab in &mut app.left.tabs {
                handle_tab(tab);
            }
            for tab in &mut app.right.tabs {
                handle_tab(tab);
            }
        }
        crate::watcher::WatcherEvent::Error(_err) => {
            // Log error to active tab error field?
            // app.left.active_tab_mut().error = Some(format!("Watcher: {}", err));
            // Don't disturb user too much
        }
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

        // Split each panel area into tab bar and content
        let left_panel_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // tab bar
                Constraint::Min(1),    // panel content
            ])
            .split(panel_chunks[0]);

        let right_panel_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // tab bar
                Constraint::Min(1),    // panel content
            ])
            .split(panel_chunks[1]);

        if app.file_viewer.is_visible && app.active == PanelSide::Right {
            crate::ui::draw_file_viewer(f, &app.file_viewer, panel_chunks[0], palette);
        } else {
            let is_active = app.active == PanelSide::Left && !app.file_viewer.focused;
            crate::ui::draw_tab_bar(f, &app.left, left_panel_layout[0], palette, is_active);
            draw_panel(
                f,
                app.left.active_tab_mut(),
                is_active,
                left_panel_layout[1],
                palette,
            );
        }

        if app.file_viewer.is_visible && app.active == PanelSide::Left {
            crate::ui::draw_file_viewer(f, &app.file_viewer, panel_chunks[1], palette);
        } else {
            let is_active = app.active == PanelSide::Right && !app.file_viewer.focused;
            crate::ui::draw_tab_bar(f, &app.right, right_panel_layout[0], palette, is_active);
            draw_panel(
                f,
                app.right.active_tab_mut(),
                is_active,
                right_panel_layout[1],
                palette,
            );
        }

        draw_panel_status(
            f,
            app.left.active_tab(),
            status_chunks[0],
            palette,
            app.active == PanelSide::Left,
            &app.task_manager,
            PanelSide::Left,
        );
        draw_panel_status(
            f,
            app.right.active_tab(),
            status_chunks[1],
            palette,
            app.active == PanelSide::Right,
            &app.task_manager,
            PanelSide::Right,
        );

        // Draw fuzzy search popup on top of everything
        crate::fuzzy_search_ui::draw_fuzzy_search_popup(f, &mut app.fuzzy_search, palette);

        // Draw rename popup on top of fuzzy search (though they shouldn't be open at same time)
        crate::rename_ui::draw_rename_popup(f, &app.popups.rename, palette);

        // Draw create directory popup
        crate::create_dir_ui::draw_create_dir_popup(f, &app.popups.create_directory, palette);
        // Draw create file popup
        crate::create_file_ui::draw_create_file_popup(f, &app.popups.create_file, palette);

        // Draw delete popup
        crate::delete_ui::draw_delete_popup(f, &app.popups.delete, palette);

        // Draw copy/move popup
        crate::copy_move_ui::draw_copy_move_popup(f, &app.popups.copy_move, palette);

        // Draw conflict popup
        crate::conflict_ui::draw_conflict_popup(f, &app.popups.conflict, palette);

        // Draw task manager
        crate::task_ui::draw_task_manager(f, &app.task_manager, app.show_task_manager, palette);

        // Draw empty trash popup
        crate::empty_trash_ui::draw_empty_trash_popup(f, &app.popups.empty_trash, palette);

        // Draw quit confirmation popup
        crate::quit_ui::draw_quit_popup(f, &app.popups.quit_confirmation, palette);

        // Draw error popup
        crate::error_ui::draw_error_popup(f, &app.popups.error, palette);

        // Draw help popup
        crate::help_ui::draw_help_popup(f, app.popups.help.is_visible, keyboard, palette);

        // Draw drive selection popup
        if app.popups.drive_select.is_visible {
            crate::drive_select_ui::draw_drive_select_popup(f, app, palette);
        }

        // Draw SSH connection popup
        if app.popups.ssh_connection.is_visible {
            crate::ssh_ui::draw_ssh_connection_popup(f, app, palette);
        }

        // Draw SSH password popup
        if app.popups.ssh_password.is_visible {
            crate::ssh_ui::draw_ssh_password_popup(f, app, palette);
        }

        // Draw remote edit confirmation popup
        crate::remote_edit_ui::draw_remote_edit_popup(f, &app.popups.remote_edit, palette);
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
            if quit_match
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
                && !app.show_task_manager
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
                if code == KeyCode::Esc {
                    app.popups.help.reset();
                }
                return false;
            }

            // Handle empty trash popup
            if app.popups.empty_trash.is_visible {
                if crate::empty_trash_ui::handle_empty_trash_popup_event(code, app) {
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
                return handle_fuzzy_search_event(code, app);
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
    let shortcut = keyevent_to_string(code, modifiers);

    // Empty Trash
    if let Some(keys) = &keyboard.empty_trash
        && keys.contains(&shortcut)
    {
        app.popups.empty_trash.is_visible = true;
        return false;
    }

    // Clear error on any interaction in the main panel
    {
        let tab_manager = match app.active {
            PanelSide::Left => &mut app.left,
            PanelSide::Right => &mut app.right,
        };
        tab_manager.active_tab_mut().error = None;
    }

    // Tab management shortcuts
    // New tab
    if let Some(keys) = &keyboard.new_tab
        && keys.contains(&shortcut)
    {
        handle_new_tab(app);
        return false;
    }
    // Next tab
    if let Some(keys) = &keyboard.tab_next
        && keys.contains(&shortcut)
    {
        handle_next_tab(app);
        return false;
    }
    // Previous tab
    if let Some(keys) = &keyboard.tab_prev
        && keys.contains(&shortcut)
    {
        handle_prev_tab(app);
        return false;
    }
    // Close tab
    if let Some(keys) = &keyboard.tab_close
        && keys.contains(&shortcut)
    {
        handle_close_tab(app);
        return false;
    }

    // Previous directory from history
    if let Some(keys) = &keyboard.back
        && keys.contains(&shortcut)
    {
        handle_history_previous(app);
        return false;
    }
    // Next directory from history
    if let Some(keys) = &keyboard.forward
        && keys.contains(&shortcut)
    {
        handle_history_next(app);
        return false;
    }
    // Enter directory
    if let Some(keys) = &keyboard.enter_dir
        && keys.contains(&shortcut)
    {
        handle_enter_directory(app);
        return false;
    }
    // Directory up
    if let Some(keys) = &keyboard.up_dir
        && keys.contains(&shortcut)
    {
        handle_directory_up(app);
        return false;
    }
    // Edit
    if let Some(keys) = &keyboard.edit_file
        && keys.contains(&shortcut)
    {
        // Await edit action, pass input_tx
        handle_edit(app, input_tx.clone()).await;
        return false;
    }
    // Fuzzy search
    if let Some(keys) = &keyboard.search
        && keys.contains(&shortcut)
    {
        app.fuzzy_search.is_visible = true;
        app.fuzzy_search.reset();
        // Initialize with all directories sorted by score
        let context_key = app.active_tab().provider.context_key();
        let results = app.dir_history.fuzzy_search(&context_key, "");
        app.fuzzy_search.filtered_dirs = results.into_iter().map(|(p, _)| p).collect();
        app.fuzzy_search.selected_index = 0;
        return false;
    }

    // Rename
    if let Some(keys) = &keyboard.rename
        && keys.contains(&shortcut)
    {
        handle_init_rename(app);
        return false;
    }

    // Create File
    if let Some(keys) = &keyboard.new_file
        && keys.contains(&shortcut)
    {
        handle_init_create_file(app);
        return false;
    }

    // Create Directory
    if let Some(keys) = &keyboard.new_dir
        && keys.contains(&shortcut)
    {
        handle_init_create_directory(app);
        return false;
    }

    // Delete
    if let Some(keys) = &keyboard.delete
        && keys.contains(&shortcut)
    {
        handle_init_delete(app, false);
        return false;
    }
    // Delete Permanently
    if let Some(keys) = &keyboard.delete_force
        && keys.contains(&shortcut)
    {
        handle_init_delete(app, true);
        return false;
    }

    // Copy
    if let Some(keys) = &keyboard.copy_to
        && keys.contains(&shortcut)
    {
        handle_init_copy(app);
        return false;
    }

    // Move
    if let Some(keys) = &keyboard.move_to
        && keys.contains(&shortcut)
    {
        handle_init_move(app);
        return false;
    }
    // Task Manager
    if let Some(keys) = &keyboard.tasks
        && keys.contains(&shortcut)
    {
        app.show_task_manager = !app.show_task_manager;
        return false;
    }

    // Help
    if let Some(keys) = &keyboard.help
        && keys.contains(&shortcut)
    {
        app.popups.help.is_visible = true;
        return false;
    }

    // Swap Tabs
    if let Some(keys) = &keyboard.swap_tabs
        && keys.contains(&shortcut)
    {
        app.swap_active_tabs();
        return false;
    }

    // Open SSH Connection
    if let Some(keys) = &keyboard.open_ssh
        && keys.contains(&shortcut)
    {
        crate::handlers::popup_ssh::handle_ssh_connection_init(app);
        return false;
    }

    // Reconnect SSH
    if let Some(keys) = &keyboard.reconnect_ssh
        && keys.contains(&shortcut)
    {
        crate::handlers::popup_ssh::handle_reconnect_ssh(app);
        return false;
    }

    // Open Terminal
    if let Some(keys) = &keyboard.open_terminal
        && keys.contains(&shortcut)
    {
        handle_open_terminal(app);
        return false;
    }

    // Drive Selection Left (Windows only)
    if let Some(keys) = &keyboard.change_drive_left
        && keys.contains(&shortcut)
    {
        let drives = crate::drive_select_ui::get_available_drives();
        if !drives.is_empty() {
            app.popups.drive_select.is_visible = true;
            app.popups.drive_select.drives = drives;
            app.popups.drive_select.side = crate::app::PanelSide::Left;
            app.popups.drive_select.selected_index = 0;
        }
        return false;
    }

    // Drive Selection Right (Windows only)
    if let Some(keys) = &keyboard.change_drive_right
        && keys.contains(&shortcut)
    {
        let drives = crate::drive_select_ui::get_available_drives();
        if !drives.is_empty() {
            app.popups.drive_select.is_visible = true;
            app.popups.drive_select.drives = drives;
            app.popups.drive_select.side = crate::app::PanelSide::Right;
            app.popups.drive_select.selected_index = 0;
        }
        return false;
    }

    // Sorting shortcuts
    if let Some(keys) = &keyboard.sort_name
        && keys.contains(&shortcut)
    {
        handle_sort(app, crate::app::SortColumn::Name);
        return false;
    }
    if let Some(keys) = &keyboard.sort_ext
        && keys.contains(&shortcut)
    {
        handle_sort(app, crate::app::SortColumn::Extension);
        return false;
    }
    if let Some(keys) = &keyboard.sort_date
        && keys.contains(&shortcut)
    {
        handle_sort(app, crate::app::SortColumn::Date);
        return false;
    }
    if let Some(keys) = &keyboard.sort_size
        && keys.contains(&shortcut)
    {
        handle_sort(app, crate::app::SortColumn::Size);
        return false;
    }
    // Select All shortcut
    if let Some(keys) = &keyboard.select_all
        && keys.contains(&shortcut)
    {
        let tab_manager = match app.active {
            crate::app::PanelSide::Left => &mut app.left,
            crate::app::PanelSide::Right => &mut app.right,
        };
        tab_manager.active_tab_mut().select_all();
        return false;
    }

    if let (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) = (code, modifiers) {
        handle_type_char(app, c);
    } else {
        let tab_manager = match app.active {
            PanelSide::Left => &mut app.left,
            PanelSide::Right => &mut app.right,
        };
        let panel = tab_manager.active_tab_mut();
        // Check if search is active (within 1 second timeout)
        let search_active = !panel.typed_buffer.is_empty()
            && panel.last_type_time.is_some_and(|t| {
                std::time::Instant::now().duration_since(t) <= std::time::Duration::from_secs(1)
            });
        match (code, modifiers) {
            (KeyCode::Tab, KeyModifiers::NONE) => handle_tab(app),
            (KeyCode::Up, _) => {
                if search_active {
                    handle_up_search(app);
                } else {
                    // Reset search if it was active but timed out
                    if !panel.typed_buffer.is_empty() {
                        reset_search(app);
                    }
                    handle_up(app);
                }
            }
            (KeyCode::Down, _) => {
                if search_active {
                    handle_down_search(app);
                } else {
                    // Reset search if it was active but timed out
                    if !panel.typed_buffer.is_empty() {
                        reset_search(app);
                    }
                    handle_down(app);
                }
            }
            (KeyCode::PageUp, _) => {
                reset_search(app);
                handle_page_up(app);
            }
            (KeyCode::PageDown, _) => {
                reset_search(app);
                handle_page_down(app);
            }
            (KeyCode::Home, _) => {
                reset_search(app);
                handle_home(app);
            }
            (KeyCode::End, _) => {
                reset_search(app);
                handle_end(app);
            }
            (KeyCode::Enter, _) => handle_open_item(app),
            (KeyCode::Esc, _) => {
                reset_search(app);
            }
            (KeyCode::Char(' '), KeyModifiers::NONE) => handle_toggle_selection(app),
            (KeyCode::Insert, _) => {
                handle_toggle_selection(app);
                handle_down(app);
            }
            _ => {}
        }
    }
    false
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
        use crate::watcher::WatcherEvent;
        let mut app = crate::app::AppState {
            left: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![crate::fs_ops::FileEntry {
                        name: "testfile.txt".to_string(),
                        is_dir: false,
                        is_symlink: false,
                        size: Some(12),
                        modified: None,
                        attributes: "".to_string(),
                        selected: false,
                    }],
                    cursor: 0,
                    history: vec![],
                    history_index: 0,
                    error: None,
                    typed_buffer: String::new(),
                    last_type_time: None,
                    matching_indices: Vec::new(),
                    search_position: 0,
                    sort_column: crate::app::SortColumn::Name,
                    sort_direction: crate::app_state::tabs::SortDirection::Ascending,
                    scroll_offset: 0,
                    custom_title: None,
                }],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: vec![],
                    history_index: 0,
                    error: None,
                    typed_buffer: String::new(),
                    last_type_time: None,
                    matching_indices: Vec::new(),
                    search_position: 0,
                    sort_column: crate::app::SortColumn::Name,
                    sort_direction: crate::app_state::tabs::SortDirection::Ascending,
                    scroll_offset: 0,
                    custom_title: None,
                }],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
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
        use crate::fs_ops::FileEntry;
        use crate::watcher::WatcherEvent;
        let mut app = crate::app::AppState {
            left: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs_local::LocalFs::new()),
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
                    history: vec![],
                    history_index: 0,
                    error: None,
                    typed_buffer: String::new(),
                    last_type_time: None,
                    matching_indices: Vec::new(),
                    search_position: 0,
                    sort_column: crate::app::SortColumn::Name,
                    sort_direction: crate::app_state::tabs::SortDirection::Ascending,
                    scroll_offset: 0,
                    custom_title: None,
                }],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: vec![],
                    history_index: 0,
                    error: None,
                    typed_buffer: String::new(),
                    last_type_time: None,
                    matching_indices: Vec::new(),
                    search_position: 0,
                    sort_column: crate::app::SortColumn::Name,
                    sort_direction: crate::app_state::tabs::SortDirection::Ascending,
                    scroll_offset: 0,
                    custom_title: None,
                }],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
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
        use crate::watcher::WatcherEvent;
        let mut app = crate::app::AppState {
            left: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: vec![],
                    history_index: 0,
                    error: None,
                    typed_buffer: String::new(),
                    last_type_time: None,
                    matching_indices: Vec::new(),
                    search_position: 0,
                    sort_column: crate::app::SortColumn::Name,
                    sort_direction: crate::app_state::tabs::SortDirection::Ascending,
                    scroll_offset: 0,
                    custom_title: None,
                }],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: vec![],
                    history_index: 0,
                    error: None,
                    typed_buffer: String::new(),
                    last_type_time: None,
                    matching_indices: Vec::new(),
                    search_position: 0,
                    sort_column: crate::app::SortColumn::Name,
                    sort_direction: crate::app_state::tabs::SortDirection::Ascending,
                    scroll_offset: 0,
                    custom_title: None,
                }],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
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
        };
        let event = WatcherEvent::Error("test error".to_string());
        super::handle_watcher_event(event, &mut app);
        // Should not panic or change error field
        assert!(app.left.active_tab().error.is_none());
    }

    #[tokio::test]
    async fn test_handle_insert_moves_cursor_down() {
        use crate::fs_ops::FileEntry;
        let mut app = crate::app::AppState {
            left: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs_local::LocalFs::new()),
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
                    history: vec![],
                    history_index: 0,
                    error: None,
                    typed_buffer: String::new(),
                    last_type_time: None,
                    matching_indices: Vec::new(),
                    search_position: 0,
                    sort_column: crate::app::SortColumn::Name,
                    sort_direction: crate::app_state::tabs::SortDirection::Ascending,
                    scroll_offset: 0,
                    custom_title: None,
                }],
                active_tab_index: 0,
            },
            right: crate::app::TabManager {
                tabs: vec![crate::app::Tab {
                    provider: std::sync::Arc::new(crate::fs_local::LocalFs::new()),
                    current_dir: std::path::PathBuf::from("/mock"),
                    entries: vec![],
                    cursor: 0,
                    history: vec![],
                    history_index: 0,
                    error: None,
                    typed_buffer: String::new(),
                    last_type_time: None,
                    matching_indices: Vec::new(),
                    search_position: 0,
                    sort_column: crate::app::SortColumn::Name,
                    sort_direction: crate::app_state::tabs::SortDirection::Ascending,
                    scroll_offset: 0,
                    custom_title: None,
                }],
                active_tab_index: 0,
            },
            active: crate::app::PanelSide::Left,
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
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
