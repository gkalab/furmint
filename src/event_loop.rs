use crate::app::{AppState, PanelSide};
use crate::config::KeyboardConfig;
use crate::theme::ThemePalette;
use crate::ui::{draw_panel, draw_panel_status};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::prelude::*;
#[cfg(not(target_os = "windows"))]
use std::env;
use std::process::Command;

pub(crate) fn spawn_input_polling(
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
                Ok(Ok(false)) => continue,
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

    let mut should_exit = false;

    // Initial draw
    draw_ui(terminal, app, palette, &keyboard)?;

    while !should_exit {
        // Explicit redraw if requested (e.g. after editor)
        if app.needs_redraw {
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

                    if let Ok(entries) = crate::fs_ops::list_dir(&tab.current_dir) {
                        tab.entries = entries;
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
        crate::watcher::WatcherEvent::Error(err) => {
            // Log error to active tab error field?
            // app.left.active_tab_mut().error = Some(format!("Watcher: {}", err));
            // Don't disturb user too much
            eprintln!("Watcher error: {err}");
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
                app.left.active_tab(),
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
                app.right.active_tab(),
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
        crate::rename_ui::draw_rename_popup(f, &app.rename_popup, palette);

        // Draw create directory popup
        crate::create_dir_ui::draw_create_dir_popup(f, &app.create_directory_popup, palette);
        // Draw create file popup
        crate::create_file_ui::draw_create_file_popup(f, &app.create_file_popup, palette);

        // Draw delete popup
        crate::delete_ui::draw_delete_popup(f, &app.delete_popup, palette);

        // Draw copy/move popup
        crate::copy_move_ui::draw_copy_move_popup(f, &app.copy_move_popup, palette);

        // Draw conflict popup
        crate::conflict_ui::draw_conflict_popup(f, &app.conflict_popup, palette);

        // Draw task manager
        crate::task_ui::draw_task_manager(f, &app.task_manager, app.show_task_manager, palette);

        // Draw empty trash popup
        crate::empty_trash_ui::draw_empty_trash_popup(f, &app.empty_trash_popup, palette);

        // Draw quit confirmation popup
        crate::quit_ui::draw_quit_popup(f, &app.quit_confirmation, palette);

        // Draw error popup
        crate::error_ui::draw_error_popup(f, &app.error_popup, palette);

        // Draw help popup
        crate::help_ui::draw_help_popup(f, app.help_popup.is_visible, keyboard, palette);
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
                && !app.rename_popup.is_visible
                && !app.create_directory_popup.is_visible
                && !app.delete_popup.is_visible
                && !app.copy_move_popup.is_visible
                && !app.conflict_popup.is_visible
                && !app.quit_confirmation.is_visible
                && !app.error_popup.is_visible
                && !app.help_popup.is_visible
                && !app.show_task_manager
            {
                if app.task_manager.has_running_tasks() {
                    app.quit_confirmation.is_visible = true;
                    return false;
                } else {
                    return true;
                }
            }

            // Handle error popup
            if app.error_popup.is_visible {
                if handle_error_popup_event(code, app).await {
                    return true;
                }
                return false;
            }

            // Handle help popup
            if app.help_popup.is_visible {
                if code == KeyCode::Esc {
                    app.help_popup.reset();
                }
                return false;
            }

            // Handle empty trash popup
            if app.empty_trash_popup.is_visible {
                if crate::empty_trash_ui::handle_empty_trash_popup_event(code, app) {
                    return false;
                }
                return false;
            }

            // Handle quit confirmation popup
            if app.quit_confirmation.is_visible {
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
            if app.rename_popup.is_visible {
                return handle_rename_popup_event(code, app);
            }

            // Handle create directory popup
            if app.create_directory_popup.is_visible {
                return handle_create_directory_popup_event(code, app);
            }

            if app.create_file_popup.is_visible {
                return handle_create_file_popup_event(code, app, &input_tx).await;
            }

            // Handle delete popup
            if app.delete_popup.is_visible {
                return handle_delete_popup_event(code, app);
            }

            // Handle copy/move popup
            if app.copy_move_popup.is_visible {
                return handle_copy_move_popup_event(code, app);
            }

            // Handle conflict popup
            if app.conflict_popup.is_visible {
                return handle_conflict_popup_event(code, app).await;
            }

            // Handle task manager
            if app.show_task_manager {
                return handle_task_manager_event(code, app);
            }

            if code == KeyCode::F(3) && modifiers == KeyModifiers::NONE {
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

            handle_main_panel_event(code, modifiers, app, keyboard, input_tx.clone()).await;
        }
        Event::Resize(_, _) => {}
        _ => {}
    }
    false
}

fn handle_quit_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            // Confirm quit
            true
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            // Cancel quit
            app.quit_confirmation.reset();
            false
        }
        _ => false,
    }
}

fn handle_fuzzy_search_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.fuzzy_search.is_visible = false;
            app.fuzzy_search.reset();
        }
        KeyCode::Enter => {
            if let Some(selected_dir) = app.fuzzy_search.get_selected_dir() {
                let tab_manager = match app.active {
                    PanelSide::Left => &mut app.left,
                    PanelSide::Right => &mut app.right,
                };
                if let Err(e) = tab_manager
                    .active_tab_mut()
                    .navigate_to(selected_dir.clone())
                {
                    tab_manager.active_tab_mut().error = Some(format!("Error: {}", e));
                } else {
                    app.dir_history.record_visit(&selected_dir);
                }
            }
            app.fuzzy_search.is_visible = false;
            app.fuzzy_search.reset();
        }
        KeyCode::Up => {
            app.fuzzy_search.move_selection_up();
        }
        KeyCode::Down => {
            app.fuzzy_search.move_selection_down();
        }
        KeyCode::PageUp => {
            app.fuzzy_search.move_selection_page_up(10);
        }
        KeyCode::PageDown => {
            app.fuzzy_search.move_selection_page_down(10);
        }
        KeyCode::Backspace => {
            app.fuzzy_search.input.pop();
            // Re-filter results
            let results = app.dir_history.fuzzy_search(&app.fuzzy_search.input);
            app.fuzzy_search.filtered_dirs = results.into_iter().map(|(p, _)| p).collect();
            app.fuzzy_search.selected_index = 0;
            app.fuzzy_search.scroll_offset = 0;
        }
        KeyCode::Char(c) => {
            app.fuzzy_search.input.push(c);
            // Re-filter results
            let results = app.dir_history.fuzzy_search(&app.fuzzy_search.input);
            app.fuzzy_search.filtered_dirs = results.into_iter().map(|(p, _)| p).collect();
            app.fuzzy_search.selected_index = 0;
            app.fuzzy_search.scroll_offset = 0;
        }
        _ => {}
    }
    false
}

fn handle_file_viewer_event(code: KeyCode, app: &mut AppState) {
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
            // Allow scrolling right (we'll handle max in rendering)
            app.file_viewer.horizontal_scroll_offset += 10;
        }
        KeyCode::PageUp => {
            let visible_rows = 20; // Approximation
            if app.file_viewer.scroll_offset >= visible_rows {
                app.file_viewer.scroll_offset -= visible_rows;
            } else {
                app.file_viewer.scroll_offset = 0;
            }
        }
        KeyCode::PageDown => {
            let visible_rows = 20; // Approximation
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
        app.empty_trash_popup.is_visible = true;
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
        let results = app.dir_history.fuzzy_search("");
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
        app.help_popup.is_visible = true;
        return false;
    }

    // Open Terminal
    if let Some(keys) = &keyboard.open_terminal
        && keys.contains(&shortcut)
    {
        handle_open_terminal(app);
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

    match (code, modifiers) {
        (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
            handle_type_char(app, c);
        }
        _ => {
            // Clear buffer for any non-letter key
            let tab_manager = match app.active {
                PanelSide::Left => &mut app.left,
                PanelSide::Right => &mut app.right,
            };
            let panel = tab_manager.active_tab_mut();
            panel.typed_buffer.clear();
            panel.last_type_time = None;
            match (code, modifiers) {
                (KeyCode::Tab, KeyModifiers::NONE) => handle_tab(app),
                (KeyCode::Up, _) => handle_up(app),
                (KeyCode::Down, _) => handle_down(app),
                (KeyCode::PageUp, _) => handle_page_up(app),
                (KeyCode::PageDown, _) => handle_page_down(app),
                (KeyCode::Home, _) => handle_home(app),
                (KeyCode::End, _) => handle_end(app),
                (KeyCode::Enter, _) => handle_enter_directory(app),
                (KeyCode::Char(' '), KeyModifiers::NONE) => handle_toggle_selection(app),
                (KeyCode::Insert, _) => handle_toggle_selection(app),
                _ => {}
            }
        }
    }
    false
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn get_default_editor() -> String {
    // First check $EDITOR, then $VISUAL
    env::var("EDITOR")
        .or_else(|_| env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_string())
}

#[cfg(target_os = "windows")]
fn get_default_editor() -> String {
    use winreg::RegKey;
    use winreg::enums::*;

    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);

    // Look up the ProgID for .txt files
    if let Ok(txt_key) = hkcr.open_subkey(".txt") {
        if let Ok(prog_id) = txt_key.get_value::<String, _>("") {
            // Open the ProgID key
            if let Ok(prog_key) = hkcr.open_subkey(&prog_id) {
                if let Ok(app) = prog_key.get_value::<String, _>("") {
                    return app;
                }
            }
        }
    }

    // Fallback if registry lookup fails
    "notepad.exe".to_string()
}

fn open_in_default_editor(file_path: &std::path::Path) -> anyhow::Result<()> {
    // Get the default editor
    let editor = get_default_editor();
    // Suspend TUI
    disable_raw_mode()?;
    std::thread::sleep(std::time::Duration::from_millis(100)); // workaround for possible terminal handoff delay
    // Launch editor as blocking subprocess
    let status = Command::new(editor).arg(file_path).status();
    // Resume TUI
    enable_raw_mode()?;
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(anyhow::anyhow!("Editor exited with status: {}", s)),
        Err(e) => Err(anyhow::anyhow!("Failed to launch editor: {}", e)),
    }
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
        KeyCode::F(n) => format!("F{}", n),
        _ => String::new(),
    };
    parts.push(key);
    parts.join("-")
}

fn handle_tab(app: &mut AppState) {
    if app.file_viewer.is_visible {
        app.file_viewer.focused = true;
    } else {
        app.active = match app.active {
            PanelSide::Left => PanelSide::Right,
            PanelSide::Right => PanelSide::Left,
        };
    }
}

fn handle_new_tab(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };

    // Create new tab at the same directory as the current tab, preserving cursor position
    let current_dir = tab_manager.active_tab().current_dir.clone();
    let cursor_pos = tab_manager.active_tab().cursor;
    if let Err(e) = tab_manager.new_tab(current_dir, Some(cursor_pos)) {
        tab_manager.active_tab_mut().error = Some(format!("Error creating tab: {}", e));
    }
}

fn handle_next_tab(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.next_tab();
    update_viewer_content(app);
}

fn handle_prev_tab(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.prev_tab();
    update_viewer_content(app);
}

fn handle_close_tab(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };

    let current_index = tab_manager.active_tab_index;
    if !tab_manager.close_tab(current_index) {
        // Could not close (last tab), optionally show a message
        // For now, just silently ignore
    }
    update_viewer_content(app);
}

fn update_viewer_content(app: &mut AppState) {
    if !app.file_viewer.is_visible {
        return;
    }
    let tab_manager = match app.active {
        PanelSide::Left => &app.left,
        PanelSide::Right => &app.right,
    };
    let panel = tab_manager.active_tab();
    if panel.entries.is_empty() {
        app.file_viewer.content = vec![];
        return;
    }
    let entry = &panel.entries[panel.cursor];
    if entry.is_dir {
        app.file_viewer.content = vec!["Directory".to_string()];
    } else {
        let full_path = panel.current_dir.join(&entry.name);
        app.file_viewer.load_content(full_path);
    }
}

fn handle_up(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_up();
    update_viewer_content(app);
}

fn handle_down(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_down();
    update_viewer_content(app);
}

fn handle_page_up(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_page_up(20);
    update_viewer_content(app);
}

fn handle_page_down(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_page_down(20);
    update_viewer_content(app);
}

fn handle_home(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_home();
    update_viewer_content(app);
}

fn handle_end(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().move_cursor_end();
    update_viewer_content(app);
}

use crossterm::event::Event as CrosstermEvent;
use tokio::sync::mpsc::UnboundedSender;

pub async fn handle_edit(app: &mut AppState, input_tx: UnboundedSender<CrosstermEvent>) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let entry = {
        let panel = tab_manager.active_tab_mut();
        panel.current_entry().cloned()
    };
    if let Some(entry) = entry {
        if !entry.is_dir {
            let file_path = {
                let panel = tab_manager.active_tab_mut();
                panel.current_dir.join(&entry.name)
            };
            let entry_name = entry.name.clone();
            let result =
                open_file_in_editor_with_env_handling(app, &file_path, Some(entry_name), &input_tx)
                    .await;
            if let Err(e) = result {
                let tab_manager = match app.active {
                    PanelSide::Left => &mut app.left,
                    PanelSide::Right => &mut app.right,
                };
                let panel = tab_manager.active_tab_mut();
                panel.error = Some(format!("Error opening editor: {}", e));
            }
        }
    }
}

fn handle_task_event(event: crate::tasks::TaskEvent, app: &mut AppState) {
    match event {
        crate::tasks::TaskEvent::UpdateStatus(id, status) => {
            app.task_manager.update_task_status(id, status);
        }
        crate::tasks::TaskEvent::UpdateProgress(id, p, t) => {
            app.task_manager.update_task_progress(id, p, t);
        }
        crate::tasks::TaskEvent::Conflict(id, path, conflict_type) => {
            // Show conflict popup
            app.conflict_popup.task_id = id;
            app.conflict_popup.conflict_path = path;
            app.conflict_popup.conflict_type = conflict_type;
            app.conflict_popup.is_visible = true;
        }
        crate::tasks::TaskEvent::Error(id, path, msg) => {
            // Show error popup
            app.error_popup.task_id = id;
            app.error_popup.error_path = path;
            app.error_popup.error_message = msg;
            app.error_popup.is_visible = true;
        }
    }
}

async fn handle_error_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    let task_id = app.error_popup.task_id;
    let decision = match code {
        KeyCode::Char('r') | KeyCode::Char('R') => Some(crate::tasks::TaskDecision::Retry),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(crate::tasks::TaskDecision::Skip),
        KeyCode::Char('a') | KeyCode::Char('A') => Some(crate::tasks::TaskDecision::SkipAll),
        KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
            Some(crate::tasks::TaskDecision::Cancel)
        }
        _ => None,
    };

    if let Some(d) = decision {
        if let Some(tx) = app.task_decision_txs.get(&task_id) {
            let _ = tx.send(d).await;
        }
        app.error_popup.reset();
    }
    false
}

fn handle_delete_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc | KeyCode::Char('n') => {
            app.delete_popup.reset();
        }
        KeyCode::Char('y') | KeyCode::Enter => {
            handle_confirm_delete(app);
            app.delete_popup.reset();
        }
        _ => {}
    }
    false
}

fn handle_confirm_delete(app: &mut AppState) {
    let paths = app.delete_popup.selected_paths.clone();
    let is_permanent = app.delete_popup.is_permanent;

    // Spawn task
    let name = if is_permanent {
        format!("Deleting {} items permanently", paths.len())
    } else {
        format!("Trashing {} items", paths.len())
    };

    app.task_manager
        .spawn_task(name, move |cancel, tx, id| async move {
            // Perform deletion
            let total = paths.len();
            let mut success = 0;
            let mut failures = Vec::new();

            for (i, path) in paths.iter().enumerate() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Cancelled,
                    ));
                    return;
                }

                let result = if is_permanent {
                    if path.is_dir() {
                        std::fs::remove_dir_all(path)
                    } else {
                        std::fs::remove_file(path)
                    }
                    .map_err(|e| e.to_string())
                } else {
                    trash::delete(path).map_err(|e| e.to_string())
                };

                match result {
                    Ok(_) => success += 1,
                    Err(e) => failures.push(format!("{}: {}", path.display(), e)),
                }

                let _ = tx.send(crate::tasks::TaskEvent::UpdateProgress(id, i + 1, total));
            }

            if failures.is_empty() {
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Completed,
                ));
            } else {
                let error_msg = if success > 0 {
                    format!("Completed with errors: {} failed", failures.len())
                } else {
                    format!("Failed: {}", failures[0]) // Show first error
                };
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Failed(error_msg),
                ));
            }
        });

    // Clear selection in active tab if deletion started
    // (Actual file removal will trigger watcher -> reload list)
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    for entry in &mut tab_manager.active_tab_mut().entries {
        entry.selected = false;
    }
}

fn handle_task_manager_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.show_task_manager = false;
        }
        KeyCode::Char('c') => {
            // Clear finished tasks
            app.task_manager.remove_finished_tasks();
        }
        KeyCode::Up => {
            app.task_manager.move_selection_up();
        }
        KeyCode::Down => {
            app.task_manager.move_selection_down();
        }
        KeyCode::Char('x') => {
            if let Some(id) = app.task_manager.get_selected_task_id() {
                app.task_manager.cancel_task(id);
                // Optionally, update status to Cancelled immediately
                app.task_manager
                    .update_task_status(id, crate::tasks::TaskStatus::Cancelled);
            }
        }
        _ => {}
    }
    false
}

fn handle_init_delete(app: &mut AppState, permanent: bool) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let tab = tab_manager.active_tab();
    let mut selected: Vec<_> = tab
        .get_selected_entries()
        .iter()
        .map(|e| tab.current_dir.join(&e.name))
        .collect();

    if selected.is_empty()
        && let Some(entry) = tab.current_entry()
        && entry.name != ".."
    {
        selected.push(tab.current_dir.join(&entry.name));
    }

    if selected.is_empty() {
        return;
    }

    app.delete_popup.selected_paths = selected;
    app.delete_popup.is_permanent = permanent;
    app.delete_popup.is_visible = true;
}

fn handle_enter_directory(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Some(entry) = panel.current_entry().cloned() {
        if entry.is_dir {
            let new_dir = if entry.name == ".." {
                panel.current_dir.parent().map(|p| p.to_path_buf())
            } else {
                Some(panel.current_dir.join(&entry.name))
            };

            if let Some(path) = new_dir {
                if let Err(e) = panel.navigate_to(path.clone()) {
                    panel.error = Some(format!("Error: {}", e));
                } else {
                    app.dir_history.record_visit(&path);
                    update_viewer_content(app);
                }
            }
        } else {
            let full_path = panel.current_dir.join(&entry.name);
            let is_exe = crate::fs_ops::is_executable(&full_path, &entry);

            if is_exe {
                // Launch executable in the default terminal
                let configured_terminal = app.global.terminal.clone();
                if let Err(e) = spawn_terminal(
                    &panel.current_dir,
                    configured_terminal,
                    Some(full_path.to_string_lossy().to_string()),
                ) {
                    panel.error = Some(format!("Error launching in terminal: {}", e));
                }
            } else {
                // Open with default application: Use xdg-open on Linux for better WM integration
                #[cfg(target_os = "linux")]
                {
                    let _ = Command::new("xdg-open")
                        .arg(&full_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
                #[cfg(not(target_os = "linux"))]
                {
                    if let Err(e) = open::that(&full_path) {
                        panel.error = Some(format!("Error opening file: {}", e));
                    }
                }
            }
        }
    }
}

fn handle_directory_up(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Some(parent) = panel.current_dir.parent() {
        let parent_path = parent.to_path_buf();
        if let Err(e) = panel.go_up() {
            panel.error = Some(format!("Error: {}", e));
        } else {
            app.dir_history.record_visit(&parent_path);
            update_viewer_content(app);
        }
    }
}

fn handle_history_previous(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Err(e) = panel.go_back() {
        panel.error = Some(format!("Error: {}", e));
    } else {
        app.dir_history.record_visit(&panel.current_dir);
        update_viewer_content(app);
    }
}

fn handle_history_next(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();

    if let Err(e) = panel.go_forward() {
        panel.error = Some(format!("Error: {}", e));
    } else {
        app.dir_history.record_visit(&panel.current_dir);
        update_viewer_content(app);
    }
}

fn handle_type_char(app: &mut AppState, c: char) {
    use std::time::Instant;
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();
    let now = Instant::now();
    let reset_threshold = std::time::Duration::from_secs(1);
    // If last_type_time is None or too old, reset buffer
    if panel
        .last_type_time
        .is_none_or(|t| now.duration_since(t) > reset_threshold)
    {
        panel.typed_buffer.clear();
    }
    panel.typed_buffer.push(c);
    panel.last_type_time = Some(now);
    let typed = panel.typed_buffer.to_lowercase();
    // Find first entry whose name starts with typed
    if let Some((idx, _)) = panel
        .entries
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name.to_lowercase().starts_with(&typed))
    {
        panel.cursor = idx;
        update_viewer_content(app);
    }
}

fn handle_toggle_selection(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().toggle_selection();
    update_viewer_content(app);
}

fn handle_sort(app: &mut AppState, column: crate::app::SortColumn) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    tab_manager.active_tab_mut().handle_sort(column);
    update_viewer_content(app);
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
}

fn handle_init_create_file(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &app.left,
        PanelSide::Right => &app.right,
    };
    let panel = tab_manager.active_tab();
    app.create_file_popup.is_visible = true;
    app.create_file_popup.input_value.clear();
    app.create_file_popup.cursor_position = 0;
    app.create_file_popup.error = None;
    app.create_file_popup.parent_dir = panel.current_dir.clone();
}

fn handle_init_rename(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &app.left,
        PanelSide::Right => &app.right,
    };
    let panel = tab_manager.active_tab();
    if let Some(entry) = panel.current_entry() {
        if entry.name == ".." {
            return;
        }
        app.rename_popup.is_visible = true;
        app.rename_popup.original_name = entry.name.clone();
        app.rename_popup.new_name = entry.name.clone();
        app.rename_popup.parent_dir = panel.current_dir.clone();
        app.rename_popup.show_overwrite_confirm = false;
        app.rename_popup.is_dir = entry.is_dir;
        app.rename_popup.error = None;

        // Position cursor before extension
        let path = std::path::Path::new(&entry.name);
        if let Some(stem) = path.file_stem() {
            app.rename_popup.cursor_position = stem.len();
        } else {
            app.rename_popup.cursor_position = entry.name.len();
        }
    }
}

fn handle_rename_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    if app.rename_popup.show_overwrite_confirm {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                perform_rename(app, true);
                app.rename_popup.reset();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.rename_popup.show_overwrite_confirm = false;
                // Pressing 'n' (not overwriting) exits the popup
                app.rename_popup.reset();
            }
            _ => {}
        }
        return false;
    }

    match code {
        KeyCode::Esc => {
            app.rename_popup.reset();
        }
        KeyCode::Enter => {
            if app.rename_popup.new_name == app.rename_popup.original_name {
                app.rename_popup.reset();
            } else {
                let new_path = app.rename_popup.parent_dir.join(&app.rename_popup.new_name);
                if new_path.exists() {
                    if app.rename_popup.is_dir {
                        app.rename_popup.error = Some("Error: Target directory exists".to_string());
                    } else {
                        app.rename_popup.show_overwrite_confirm = true;
                    }
                } else {
                    perform_rename(app, false);
                    app.rename_popup.reset();
                }
            }
        }
        KeyCode::Char(c) => {
            app.rename_popup.error = None; // Clear error on typing
            app.rename_popup
                .new_name
                .insert(app.rename_popup.cursor_position, c);
            app.rename_popup.cursor_position += 1;
        }
        KeyCode::Backspace => {
            app.rename_popup.error = None; // Clear error on typing
            if app.rename_popup.cursor_position > 0 {
                app.rename_popup
                    .new_name
                    .remove(app.rename_popup.cursor_position - 1);
                app.rename_popup.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if app.rename_popup.cursor_position < app.rename_popup.new_name.len() {
                app.rename_popup
                    .new_name
                    .remove(app.rename_popup.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.rename_popup.cursor_position > 0 {
                app.rename_popup.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.rename_popup.cursor_position < app.rename_popup.new_name.len() {
                app.rename_popup.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.rename_popup.cursor_position = 0;
        }
        KeyCode::End => {
            app.rename_popup.cursor_position = app.rename_popup.new_name.len();
        }
        _ => {}
    }
    false
}

fn perform_rename(app: &mut AppState, overwrite: bool) {
    let old_path = app
        .rename_popup
        .parent_dir
        .join(&app.rename_popup.original_name);
    let new_path = app.rename_popup.parent_dir.join(&app.rename_popup.new_name);

    // If overwrite is true and target exists, we might need to remove it first or just rename over it.
    // std::fs::rename overwrites on Unix, but on Windows it might fail if target exists.
    // For safety and cross-platform consistency, if we confirmed overwrite, we can try rename directly.
    // If it fails because it exists (Windows), we might need to remove target first.
    // But std::fs::rename documentation says: "This function will replace the destination if it already exists." on Unix.
    // On Windows: "This function will return an error if to already exists."

    let result = if overwrite && cfg!(target_os = "windows") && new_path.exists() {
        std::fs::remove_file(&new_path).and_then(|_| std::fs::rename(&old_path, &new_path))
    } else {
        std::fs::rename(&old_path, &new_path)
    };

    match result {
        Ok(_) => {
            // Refresh the active panel
            let tab_manager = match app.active {
                PanelSide::Left => &mut app.left,
                PanelSide::Right => &mut app.right,
            };
            let panel = tab_manager.active_tab_mut();

            // Refresh entries
            match crate::fs_ops::list_dir(&panel.current_dir) {
                Ok(entries) => {
                    panel.entries = entries;
                    panel.sort_entries();
                    // Try to select the renamed file
                    if let Some(idx) = panel
                        .entries
                        .iter()
                        .position(|e| e.name == app.rename_popup.new_name)
                    {
                        panel.cursor = idx;
                    }
                }
                Err(e) => {
                    panel.error = Some(format!("Error refreshing directory: {}", e));
                }
            }
        }
        Err(e) => {
            let tab_manager = match app.active {
                PanelSide::Left => &mut app.left,
                PanelSide::Right => &mut app.right,
            };
            tab_manager.active_tab_mut().error = Some(format!("Error renaming: {}", e));
        }
    }
}

fn handle_init_copy(app: &mut AppState) {
    init_copy_move(app, crate::app::CopyMoveAction::Copy);
}

fn handle_init_move(app: &mut AppState) {
    init_copy_move(app, crate::app::CopyMoveAction::Move);
}

fn init_copy_move(app: &mut AppState, action: crate::app::CopyMoveAction) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let tab = tab_manager.active_tab();
    let selected: Vec<_> = tab
        .get_selected_entries()
        .iter()
        .map(|e| tab.current_dir.join(&e.name))
        .collect();

    let paths = if selected.is_empty() {
        if let Some(entry) = tab.current_entry() {
            if entry.name != ".." {
                vec![tab.current_dir.join(&entry.name)]
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    } else {
        selected
    };

    if paths.is_empty() {
        return;
    }

    // Get inactive panel path
    let inactive_tab = match app.active {
        PanelSide::Left => app.right.active_tab(),
        PanelSide::Right => app.left.active_tab(),
    };
    let dest = inactive_tab.current_dir.to_string_lossy().to_string();

    app.copy_move_popup.source_paths = paths;
    app.copy_move_popup.action = action;
    app.copy_move_popup.destination_input = dest;
    app.copy_move_popup.cursor_position = app.copy_move_popup.destination_input.len();
    app.copy_move_popup.input_selected = false;
    app.copy_move_popup.is_visible = true;
}

fn handle_copy_move_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.copy_move_popup.reset();
        }
        KeyCode::Enter => {
            // Validation
            let dest_input = app.copy_move_popup.destination_input.clone();
            // Handle tilde expansion if needed
            let dest_path = if dest_input.starts_with("~") {
                if let Some(base_dirs) = directories::BaseDirs::new() {
                    let home = base_dirs.home_dir();
                    if dest_input == "~" {
                        home.to_path_buf()
                    } else {
                        home.join(dest_input.trim_start_matches("~/"))
                    }
                } else {
                    std::path::PathBuf::from(dest_input)
                }
            } else {
                std::path::PathBuf::from(dest_input)
            };

            // Canonicalize dest if possible to resolving symlinks/relativity for accurate check
            // If it doesn't exist yet, we check parent
            let dest_abs = if let Ok(p) = dest_path.canonicalize() {
                p
            } else {
                // Try determining absolute path relative to current dir?
                // dest_input is usually absolute path from UI default.
                if dest_path.is_absolute() {
                    dest_path
                } else {
                    // This case might happen if user types relative path "foo/bar"
                    // Relative to Active Panel directory (Source directory)
                    match app.active {
                        PanelSide::Left => app.left.active_tab().current_dir.join(&dest_path),
                        PanelSide::Right => app.right.active_tab().current_dir.join(&dest_path),
                    }
                }
            };

            // Update input to absolute path so spawn_task uses the correct path
            app.copy_move_popup.destination_input = dest_abs.to_string_lossy().to_string();

            for src in &app.copy_move_popup.source_paths {
                // Canonicalize src
                if let Ok(src_abs) = src.canonicalize() {
                    // Check 1: Destination IS Source (e.g. cp /a/b to /a/b)
                    if src_abs == dest_abs {
                        app.copy_move_popup.error =
                            Some("Cannot copy/move source into itself".to_string());
                        return false;
                    }
                    // Check 2: Destination is INSIDE Source (e.g. cp /a to /a/b)
                    if dest_abs.starts_with(&src_abs) {
                        app.copy_move_popup.error =
                            Some("Cannot copy/move into subdirectory of itself".to_string());
                        return false;
                    }

                    // Check 3: Effective Destination IS Source (e.g. cp /a/b to /a)
                    // If we copy /a/b to /a, the result is /a/b, which IS /a/b.
                    if let Some(file_name) = src_abs.file_name() {
                        let effective_dest = dest_abs.join(file_name);
                        if effective_dest == src_abs {
                            app.copy_move_popup.error =
                                Some("Source and destination are the same".to_string());
                            return false;
                        }
                    }
                }
            }

            spawn_copy_move_task(app);
            app.copy_move_popup.reset();
        }
        KeyCode::Char(c) => {
            app.copy_move_popup.error = None; // Clear error on type
            app.copy_move_popup
                .destination_input
                .insert(app.copy_move_popup.cursor_position, c);
            app.copy_move_popup.cursor_position += 1;
        }
        KeyCode::Backspace => {
            if app.copy_move_popup.cursor_position > 0 {
                app.copy_move_popup
                    .destination_input
                    .remove(app.copy_move_popup.cursor_position - 1);
                app.copy_move_popup.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if app.copy_move_popup.cursor_position < app.copy_move_popup.destination_input.len() {
                app.copy_move_popup
                    .destination_input
                    .remove(app.copy_move_popup.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.copy_move_popup.cursor_position > 0 {
                app.copy_move_popup.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.copy_move_popup.cursor_position < app.copy_move_popup.destination_input.len() {
                app.copy_move_popup.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.copy_move_popup.cursor_position = 0;
        }
        KeyCode::End => {
            app.copy_move_popup.cursor_position = app.copy_move_popup.destination_input.len();
        }
        _ => {}
    }
    false
}

async fn handle_conflict_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    let task_id = app.conflict_popup.task_id;
    let decision = match code {
        KeyCode::Char('o') | KeyCode::Char('O') => Some(crate::tasks::TaskDecision::Overwrite),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(crate::tasks::TaskDecision::Skip),
        KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
            Some(crate::tasks::TaskDecision::Cancel)
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(crate::tasks::TaskDecision::OverwriteAll),
        KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Char('n') | KeyCode::Char('N') => {
            Some(crate::tasks::TaskDecision::SkipAll)
        }
        KeyCode::Char('m') | KeyCode::Char('M') => Some(crate::tasks::TaskDecision::Merge),
        _ => None,
    };

    if let Some(d) = decision {
        if let Some(tx) = app.task_decision_txs.get(&task_id) {
            let _ = tx.send(d).await;
        }
        // Reset popup immediately, task will continue
        app.conflict_popup.reset();
    }
    false
}

fn spawn_copy_move_task(app: &mut AppState) {
    let paths = app.copy_move_popup.source_paths.clone();
    let dest_str = app.copy_move_popup.destination_input.clone();
    let action = app.copy_move_popup.action;

    // Validate destination
    let dest_path = std::path::PathBuf::from(&dest_str);

    let task_name = match action {
        crate::app::CopyMoveAction::Copy => format!("Copying {} items", paths.len()),
        crate::app::CopyMoveAction::Move => format!("Moving {} items", paths.len()),
    };

    // Deselect files in active panel
    {
        let entries = match app.active {
            crate::app::PanelSide::Left => &mut app.left.active_tab_mut().entries,
            crate::app::PanelSide::Right => &mut app.right.active_tab_mut().entries,
        };
        for entry in entries.iter_mut() {
            if entry.selected {
                entry.selected = false;
            }
        }
    }

    // Create channel for decisions
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(1);

    let id = app
        .task_manager
        .spawn_task(task_name, move |cancel, tx, id| async move {
            // Pre-calculation of total items (approximate)
            let total_items = count_items(&paths);
            let processed_items = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

            // State for "Apply to all" decisions
            // We use a struct to hold this state across recursions
            let mut decision_state = DecisionState {
                overwrite_all: false,
                skip_all: false,

                last_update: std::time::Instant::now(),
            };

            // We need `decision_rx` to be mutual, so we wrap it
            let decision_rx = std::sync::Arc::new(tokio::sync::Mutex::new(decision_rx));

            // Ensure dest dir exists if multiple items or if treated as dir
            let treat_as_dir = paths.len() > 1
                || dest_path.is_dir()
                || dest_str.ends_with(std::path::MAIN_SEPARATOR);

            if treat_as_dir {
                if let Err(e) = tokio::fs::create_dir_all(&dest_path).await {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Failed(e.to_string()),
                    ));
                    return;
                }
            } else if let Some(parent) = dest_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            let mut failures = Vec::new();

            for src in &paths {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Cancelled,
                    ));
                    return;
                }

                let file_name = match src.file_name() {
                    Some(n) => n,
                    None => continue,
                };

                let target = if treat_as_dir {
                    dest_path.join(file_name)
                } else {
                    dest_path.clone()
                };

                // Recursive copy/move
                let res = recursive_op(
                    src,
                    &target,
                    action,
                    &cancel,
                    &tx,
                    id,
                    total_items,
                    &processed_items,
                    &decision_rx,
                    &mut decision_state,
                )
                .await;

                if let Err(e) = res {
                    failures.push(e);
                }
            }

            if failures.is_empty() {
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Completed,
                ));
            } else {
                // ... error handling
                let msg = format!("Failed with {} errors", failures.len());
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Failed(msg),
                ));
            }
        });

    // Store decision tx
    app.task_decision_txs.insert(id, decision_tx);
}

// Helper to count items recursively
fn count_items(paths: &[std::path::PathBuf]) -> usize {
    let mut count = 0;
    for path in paths {
        count += 1; // Count the item itself
        if path.is_dir()
            && let Ok(entries) = std::fs::read_dir(path)
        {
            let mut children = Vec::new();
            for entry in entries.flatten() {
                children.push(entry.path());
            }
            count += count_items(&children);
        }
    }
    count
}

struct DecisionState {
    overwrite_all: bool,
    skip_all: bool,

    last_update: std::time::Instant,
}

// Recursive operation
// Returns Result<(), String>
// Recursive operation
// Returns Result<(), String>
// Recursive operation
// Returns Result<(), String>
// Iterative operation to avoid stack overflow
// Returns Result<(), String>
fn recursive_op<'a>(
    src: &'a std::path::Path,
    dest: &'a std::path::Path,
    action: crate::app::CopyMoveAction,
    cancel: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: &'a tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    id: usize,
    total: usize,
    processed: &'a std::sync::Arc<std::sync::atomic::AtomicUsize>,
    decision_rx: &'a std::sync::Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
    >,
    decision_state: &'a mut DecisionState,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        // Internal enum for stack
        enum WorkItem {
            Process {
                src: std::path::PathBuf,
                dest: std::path::PathBuf,
            },
            PostProcessDir {
                src: std::path::PathBuf,
            },
        }

        let mut stack = vec![WorkItem::Process {
            src: src.to_path_buf(),
            dest: dest.to_path_buf(),
        }];

        while let Some(item) = stack.pop() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(()); // Cancelled
            }

            match item {
                WorkItem::PostProcessDir { src } => {
                    // Remove empty directory after move
                    let _ = tokio::fs::remove_dir(src).await;
                }
                WorkItem::Process { src, dest } => {
                    // Move optimization: Try rename first if it's a move operation
                    if action == crate::app::CopyMoveAction::Move {
                        // Only try rename if dest doesn't exist to avoid implicit overwrite
                        if let Ok(false) = tokio::fs::try_exists(&dest).await
                            && tokio::fs::rename(&src, &dest).await.is_ok()
                        {
                            // Success, no need to process children or post-process
                            continue;
                        }
                    }

                    if src.is_dir() {
                        // Directory handling
                        let dest_exists = tokio::fs::try_exists(&dest).await.unwrap_or(false);

                        if !dest_exists {
                            if let Err(e) = tokio::fs::create_dir_all(&dest).await {
                                return Err(format!(
                                    "Failed to create directory {}: {}",
                                    dest.display(),
                                    e
                                ));
                            }
                        } else if !dest.is_dir() {
                            return Err(format!(
                                "Destination {} exists and is not a directory",
                                dest.display()
                            ));
                        }

                        // If Move, we need to remove this dir AFTER processing children
                        if action == crate::app::CopyMoveAction::Move {
                            stack.push(WorkItem::PostProcessDir { src: src.clone() });
                        }

                        // Read children
                        let mut entries = match tokio::fs::read_dir(&src).await {
                            Ok(e) => e,
                            Err(e) => {
                                return Err(format!(
                                    "Failed to read directory {}: {}",
                                    src.display(),
                                    e
                                ));
                            }
                        };

                        while let Ok(Some(entry)) = entries.next_entry().await {
                            let path = entry.path();
                            let name = match path.file_name() {
                                Some(n) => n,
                                None => continue,
                            };
                            let child_dest = dest.join(name);
                            stack.push(WorkItem::Process {
                                src: path,
                                dest: child_dest,
                            });
                        }
                    } else {
                        // File handling
                        let mut perform = true;
                        let dest_exists = tokio::fs::try_exists(&dest).await.unwrap_or(false);

                        if dest_exists {
                            // Conflict resolution
                            if decision_state.overwrite_all {
                                perform = true;
                            } else if decision_state.skip_all {
                                perform = false;
                            } else {
                                // Ask user
                                let _ = tx.send(crate::tasks::TaskEvent::Conflict(
                                    id,
                                    dest.clone(),
                                    crate::tasks::ConflictType::FileExists,
                                ));

                                // Wait for decision
                                let mut decision = None;
                                if let Some(rx) = decision_rx.try_lock().ok().as_mut() {
                                    // We need to wait for a decision.
                                    // NOTE: This blocks the async task, but that's what we want.
                                    // The UI runs in a separate thread/event loop.
                                    decision = rx.recv().await;
                                }

                                match decision {
                                    Some(crate::tasks::TaskDecision::Overwrite) => perform = true,
                                    Some(crate::tasks::TaskDecision::OverwriteAll) => {
                                        decision_state.overwrite_all = true;
                                        perform = true;
                                    }
                                    Some(crate::tasks::TaskDecision::Skip) => perform = false,
                                    Some(crate::tasks::TaskDecision::SkipAll) => {
                                        decision_state.skip_all = true;
                                        perform = false;
                                    }
                                    Some(crate::tasks::TaskDecision::Cancel) => return Ok(()),
                                    _ => perform = false, // Default skip or error
                                }
                            }
                        }

                        if perform {
                            loop {
                                if dest_exists {
                                    // Try to remove destination if it exists (overwrite)
                                    let _ = tokio::fs::remove_file(&dest).await;
                                }

                                match tokio::fs::copy(&src, &dest).await {
                                    Ok(_) => break, // Success
                                    Err(e) => {
                                        // Check SkipAll flag
                                        if decision_state.skip_all {
                                            perform = false;
                                            break;
                                        }

                                        // Ask user
                                        let _ = tx.send(crate::tasks::TaskEvent::Error(
                                            id,
                                            src.display().to_string(),
                                            format!("Failed to copy to {}: {}", dest.display(), e),
                                        ));

                                        // Wait for decision
                                        let mut decision = None;
                                        if let Some(rx) = decision_rx.try_lock().ok().as_mut() {
                                            decision = rx.recv().await;
                                        }

                                        match decision {
                                            Some(crate::tasks::TaskDecision::Retry) => continue, // Retry loop
                                            Some(crate::tasks::TaskDecision::Skip) => {
                                                perform = false;
                                                break;
                                            }
                                            Some(crate::tasks::TaskDecision::SkipAll) => {
                                                decision_state.skip_all = true;
                                                perform = false;
                                                break;
                                            }
                                            Some(crate::tasks::TaskDecision::Cancel) => {
                                                return Ok(());
                                            }
                                            _ => {
                                                perform = false;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // If Move AND perform was success (or if we skipped, we usually DON'T delete source?
                        // Wait, if we 'Skip', we shouldn't delete source in a Move.
                        // Standard Move behavior: if we copy successfully, we delete source.
                        // If we skip copying, the source remains.
                        // So only delete source if perform was true AND success.
                        if action == crate::app::CopyMoveAction::Move && perform {
                            let _ = tokio::fs::remove_file(&src).await;
                        }

                        // Update progress
                        let p = processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let now = std::time::Instant::now();
                        if now.duration_since(decision_state.last_update)
                            > std::time::Duration::from_millis(100)
                            || p == total
                        {
                            let _ = tx.send(crate::tasks::TaskEvent::UpdateProgress(id, p, total));
                            decision_state.last_update = now;
                        }
                    }
                }
            }
        }
        Ok(())
    })
}

fn handle_init_create_directory(app: &mut AppState) {
    app.create_directory_popup.reset();
    app.create_directory_popup.is_visible = true;
}

fn handle_create_directory_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.create_directory_popup.is_visible = false;
            app.create_directory_popup.reset();
        }
        KeyCode::Enter => {
            let new_name = app.create_directory_popup.new_name.trim().to_string();
            if new_name.is_empty() {
                return false;
            }

            let tab_manager = match app.active {
                PanelSide::Left => &mut app.left,
                PanelSide::Right => &mut app.right,
            };
            let current_dir = &tab_manager.active_tab().current_dir;
            let new_path = current_dir.join(&new_name);

            match crate::fs_ops::create_directory(&new_path) {
                Ok(_) => {
                    app.create_directory_popup.is_visible = false;
                    app.create_directory_popup.reset();
                    // Reload active tab
                    if let Ok(entries) = crate::fs_ops::list_dir(current_dir) {
                        tab_manager.active_tab_mut().entries = entries;
                        tab_manager.active_tab_mut().sort_entries();

                        // Try to select the new directory
                        if let Some(name) = new_path.file_name().and_then(|n| n.to_str())
                            && let Some(idx) = tab_manager
                                .active_tab()
                                .entries
                                .iter()
                                .position(|e| e.name == name)
                        {
                            tab_manager.active_tab_mut().cursor = idx;
                        }
                    }
                }
                Err(e) => {
                    app.create_directory_popup.error = Some(e.to_string());
                }
            }
        }
        KeyCode::Backspace => {
            if app.create_directory_popup.cursor_position > 0 {
                let current_len = app.create_directory_popup.new_name.chars().count();
                if app.create_directory_popup.cursor_position <= current_len {
                    // Remove char at cursor_position - 1
                    let byte_idx = app
                        .create_directory_popup
                        .new_name
                        .char_indices()
                        .nth(app.create_directory_popup.cursor_position - 1)
                        .map(|(i, _)| i)
                        .unwrap();
                    app.create_directory_popup.new_name.remove(byte_idx);
                    app.create_directory_popup.cursor_position -= 1;
                }
            }
        }
        KeyCode::Delete => {
            let current_len = app.create_directory_popup.new_name.chars().count();
            if app.create_directory_popup.cursor_position < current_len {
                let byte_idx = app
                    .create_directory_popup
                    .new_name
                    .char_indices()
                    .nth(app.create_directory_popup.cursor_position)
                    .map(|(i, _)| i)
                    .unwrap();
                app.create_directory_popup.new_name.remove(byte_idx);
            }
        }
        KeyCode::Left => {
            if app.create_directory_popup.cursor_position > 0 {
                app.create_directory_popup.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            let len = app.create_directory_popup.new_name.chars().count();
            if app.create_directory_popup.cursor_position < len {
                app.create_directory_popup.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.create_directory_popup.cursor_position = 0;
        }
        KeyCode::End => {
            app.create_directory_popup.cursor_position =
                app.create_directory_popup.new_name.chars().count();
        }
        KeyCode::Char(c) => {
            let idx = app.create_directory_popup.cursor_position;
            // Insert at cursor position
            if idx >= app.create_directory_popup.new_name.chars().count() {
                app.create_directory_popup.new_name.push(c);
            } else {
                let byte_idx = app
                    .create_directory_popup
                    .new_name
                    .char_indices()
                    .nth(idx)
                    .map(|(i, _)| i)
                    .unwrap();
                app.create_directory_popup.new_name.insert(byte_idx, c);
            }
            app.create_directory_popup.cursor_position += 1;
        }
        _ => {}
    }
    // Clear error on any input in the popup
    if code != KeyCode::Enter && app.create_directory_popup.error.is_some() {
        app.create_directory_popup.error = None;
    }
    false
}

pub async fn handle_create_file_popup_event(
    code: KeyCode,
    app: &mut AppState,
    input_tx: &tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> bool {
    use std::io::Write;
    use std::path::Path;
    match code {
        KeyCode::Esc => {
            app.create_file_popup.reset();
        }
        KeyCode::Enter => {
            app.create_file_popup.error = None;
            let input = app.create_file_popup.input_value.trim();
            if input.is_empty() {
                app.create_file_popup.error = Some("File name cannot be empty".to_string());
                return false;
            }
            let path_buf = if input.starts_with("~") {
                if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
                {
                    if input == "~" {
                        home
                    } else if input.starts_with("~/") {
                        home.join(&input[2..])
                    } else {
                        app.create_file_popup.error =
                            Some("Unsupported ~username syntax".to_string());
                        return false;
                    }
                } else {
                    app.create_file_popup.error =
                        Some("Cannot resolve ~ to home directory".to_string());
                    return false;
                }
            } else {
                let p = Path::new(input);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    app.create_file_popup.parent_dir.join(p)
                }
            };
            // Disallow creating a directory and special names
            if path_buf.as_os_str().is_empty() || path_buf.ends_with("/") || path_buf.is_dir() {
                app.create_file_popup.error = Some("Invalid file name".to_string());
                return false;
            }
            // File must not already exist
            if path_buf.exists() {
                app.create_file_popup.error =
                    Some("A file with that name already exists".to_string());
                return false;
            }
            // Try to create empty file atomically
            let create_result = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path_buf);
            match create_result {
                Ok(mut f) => {
                    // file will be truncated, nothing to write; drop after this scope
                    if let Err(e) = f.flush() {
                        app.create_file_popup.error = Some(format!("Error writing file: {}", e));
                        return false;
                    }
                    drop(f);
                }
                Err(e) => {
                    app.create_file_popup.error = Some(format!("Failed to create file: {}", e));
                    return false;
                }
            };
            // Open in editor using environment helper
            let file_name_opt = path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
            let editor_result =
                open_file_in_editor_with_env_handling(app, &path_buf, file_name_opt, input_tx)
                    .await;
            if let Err(e) = editor_result {
                app.create_file_popup.error = Some(format!("Failed to open in editor: {}", e));
                return false;
            }
            app.create_file_popup.reset();
        }
        KeyCode::Char(c) => {
            app.create_file_popup.error = None;
            app.create_file_popup
                .input_value
                .insert(app.create_file_popup.cursor_position, c);
            app.create_file_popup.cursor_position += 1;
        }
        KeyCode::Backspace => {
            app.create_file_popup.error = None;
            if app.create_file_popup.cursor_position > 0 {
                app.create_file_popup
                    .input_value
                    .remove(app.create_file_popup.cursor_position - 1);
                app.create_file_popup.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            app.create_file_popup.error = None;
            if app.create_file_popup.cursor_position < app.create_file_popup.input_value.len() {
                app.create_file_popup
                    .input_value
                    .remove(app.create_file_popup.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.create_file_popup.cursor_position > 0 {
                app.create_file_popup.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.create_file_popup.cursor_position < app.create_file_popup.input_value.len() {
                app.create_file_popup.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.create_file_popup.cursor_position = 0;
        }
        KeyCode::End => {
            app.create_file_popup.cursor_position = app.create_file_popup.input_value.len();
        }
        _ => {}
    }
    false
}

pub async fn open_file_in_editor_with_env_handling(
    app: &mut AppState,
    file_path: &std::path::Path,
    filename_to_select: Option<String>,
    input_tx: &tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> anyhow::Result<()> {
    // 1. Abort input polling
    if let Some(handle) = app.input_polling_handle.take() {
        handle.abort();
    }
    // 2. Pause watcher
    let panel_current_dir = {
        let tab_manager = match app.active {
            PanelSide::Left => &mut app.left,
            PanelSide::Right => &mut app.right,
        };
        let panel = tab_manager.active_tab_mut();
        panel.current_dir.clone()
    };
    if let Some(watcher) = &mut app.watcher {
        let paths = watcher.watched_paths.clone();
        for path in &paths {
            let _ = watcher.unwatch(path);
        }
    }
    // 3. Run editor
    let result = tokio::task::spawn_blocking({
        let path = file_path.to_path_buf();
        move || open_in_default_editor(&path)
    })
    .await;
    let err = match result {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(format!("Error opening editor: {}", e)),
        Err(e) => Some(format!("Error launching editor: {}", e)),
    };
    // 4. Restart watcher
    if let Some(watcher) = &mut app.watcher {
        let _ = watcher.watch(&panel_current_dir);
    }
    app.sync_watcher();
    // 5. Refresh file list and cursor
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();
    if let Ok(entries) = crate::fs_ops::list_dir(&panel_current_dir) {
        panel.entries = entries;
        panel.sort_entries();
        if let Some(name) = filename_to_select {
            if let Some(idx) = panel.entries.iter().position(|e| e.name == name) {
                panel.cursor = idx;
            } else if panel.cursor >= panel.entries.len() {
                panel.cursor = panel.entries.len().saturating_sub(1);
            }
        }
    }
    // 6. Restart input polling
    app.input_polling_handle = Some(spawn_input_polling(input_tx.clone()));
    app.needs_redraw = true;
    // 7. Return error or success
    if let Some(e) = err {
        Err(anyhow::anyhow!(e))
    } else {
        Ok(())
    }
}
pub fn handle_open_terminal(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let current_dir = tab_manager.active_tab().current_dir.clone();
    let configured_terminal = app.global.terminal.clone();

    if let Err(e) = spawn_terminal(&current_dir, configured_terminal, None) {
        tab_manager.active_tab_mut().error = Some(format!("Error opening terminal: {}", e));
    }
}
fn spawn_terminal(
    dir: &std::path::Path,
    configured_terminal: Option<String>,
    command: Option<String>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let terminal_list = if let Some(term) = configured_terminal {
            vec![term]
        } else {
            vec![
                "x-terminal-emulator".to_string(),
                "gnome-terminal".to_string(),
                "konsole".to_string(),
                "xfce4-terminal".to_string(),
                "alacritty".to_string(),
                "kitty".to_string(),
                "foot".to_string(),
                "termite".to_string(),
                "st".to_string(),
                "xterm".to_string(),
                "urxvt".to_string(),
            ]
        };

        for term in terminal_list {
            let mut cmd = Command::new(&term);
            cmd.current_dir(dir);

            if let Some(ref c) = command {
                // Most Linux terminals use -e to execute a command
                // Some might need special handling, but -e is the most universal
                cmd.arg("-e").arg(c);
            }

            // Check if terminal exists before spawning
            if Command::new(&term).arg("--version").output().is_ok()
                || term == "x-terminal-emulator"
            {
                cmd.spawn()?;
                return Ok(());
            }
        }
        Err(anyhow::anyhow!("No suitable terminal emulator found"))
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(c) = command {
            Command::new("open")
                .arg("-a")
                .arg("Terminal")
                .arg("-e")
                .arg(c)
                .current_dir(dir)
                .spawn()?;
        } else if let Some(term) = configured_terminal {
            Command::new("open").arg("-a").arg(term).arg(dir).spawn()?;
        } else {
            Command::new("open")
                .arg("-a")
                .arg("Terminal")
                .arg(dir)
                .spawn()?;
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(c) = command {
            Command::new("cmd")
                .arg("/c")
                .arg("start")
                .arg(if let Some(term) = configured_terminal {
                    term
                } else {
                    "cmd".to_string()
                })
                .arg("/k") // Keep terminal open after command
                .arg(c)
                .current_dir(dir)
                .spawn()?;
        } else if let Some(term) = configured_terminal {
            Command::new("cmd")
                .arg("/c")
                .arg("start")
                .arg(term)
                .current_dir(dir)
                .spawn()?;
        } else {
            Command::new("cmd")
                .arg("/c")
                .arg("start")
                .arg("cmd")
                .current_dir(dir)
                .spawn()?;
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    Err(anyhow::anyhow!("Unsupported OS"))
}
