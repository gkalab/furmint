use crate::app::{AppState, PanelSide};
use crate::config::KeyboardConfig;
use crate::theme::ThemePalette;
use crate::ui::{draw_panel, draw_panel_status};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::prelude::*;
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
    draw_ui(terminal, app, palette)?;

    while !should_exit {
        tokio::select! {
                            // Handle watcher events
                            Some(event) = watcher_rx.recv() => {
                                handle_watcher_event(event, app);
                                app.sync_watcher();
                                draw_ui(terminal, app, palette)?;
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
                                    draw_ui(terminal, app, palette)?;
                                }

                                // Sync watcher if navigation happened
                                app.sync_watcher();
                            }
                            // Handle task events
                            Some(event) = task_rx.recv() => {
                                handle_task_event(event, app);
                                draw_ui(terminal, app, palette)?;
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
        );
        draw_panel_status(
            f,
            app.right.active_tab(),
            status_chunks[1],
            palette,
            app.active == PanelSide::Right,
        );

        // Draw fuzzy search popup on top of everything
        crate::fuzzy_search_ui::draw_fuzzy_search_popup(f, &mut app.fuzzy_search, palette);

        // Draw rename popup on top of fuzzy search (though they shouldn't be open at same time)
        crate::rename_ui::draw_rename_popup(f, &app.rename_popup, palette);

        // Draw delete popup
        crate::delete_ui::draw_delete_popup(f, &app.delete_popup, palette);

        // Draw task manager
        crate::task_ui::draw_task_manager(f, &app.task_manager, app.show_task_manager, palette);

        // Draw task status in status bar area (overlay or append)
        // We can draw it over the right status panel if tasks are running
        if !app.show_task_manager {
            let status_area = status_chunks[1];
            // Align to right side of status area?
            // For now just draw it
            crate::task_ui::draw_task_status_bar(f, &app.task_manager, status_area, palette);
        }
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
            if (code == KeyCode::Char('q') && modifiers == KeyModifiers::CONTROL)
                || (code == KeyCode::Esc
                    && !app.file_viewer.is_visible
                    && !app.fuzzy_search.is_visible
                    && !app.rename_popup.is_visible
                    && !app.delete_popup.is_visible
                    && !app.show_task_manager)
            {
                return true;
            }

            // Handle fuzzy search popup
            if app.fuzzy_search.is_visible {
                return handle_fuzzy_search_event(code, app);
            }

            // Handle rename popup
            if app.rename_popup.is_visible {
                return handle_rename_popup_event(code, app);
            }

            // Handle delete popup
            if app.delete_popup.is_visible {
                return handle_delete_popup_event(code, app);
            }

            // Handle task manager
            if app.show_task_manager {
                return handle_task_manager_event(code, app);
            }

            if (code == KeyCode::F(3) && modifiers == KeyModifiers::NONE) || code == KeyCode::Esc {
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
    if let Some(keys) = &keyboard.next_tab
        && keys.contains(&shortcut)
    {
        handle_next_tab(app);
        return false;
    }
    // Previous tab
    if let Some(keys) = &keyboard.prev_tab
        && keys.contains(&shortcut)
    {
        handle_prev_tab(app);
        return false;
    }
    // Close tab
    if let Some(keys) = &keyboard.close_tab
        && keys.contains(&shortcut)
    {
        handle_close_tab(app);
        return false;
    }

    // Previous directory from history
    if let Some(keys) = &keyboard.history_previous
        && keys.contains(&shortcut)
    {
        handle_history_previous(app);
        return false;
    }
    // Next directory from history
    if let Some(keys) = &keyboard.history_next
        && keys.contains(&shortcut)
    {
        handle_history_next(app);
        return false;
    }
    // Enter directory
    if let Some(keys) = &keyboard.enter_directory
        && keys.contains(&shortcut)
    {
        handle_enter_directory(app);
        return false;
    }
    // Directory up
    if let Some(keys) = &keyboard.directory_up
        && keys.contains(&shortcut)
    {
        handle_directory_up(app);
        return false;
    }
    // Edit
    if let Some(keys) = &keyboard.edit
        && keys.contains(&shortcut)
    {
        // Await edit action, pass input_tx
        handle_edit(app, input_tx.clone()).await;
        return false;
    }
    // Fuzzy search
    if let Some(keys) = &keyboard.fuzzy_search
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

    // Delete
    if let Some(keys) = &keyboard.delete
        && keys.contains(&shortcut)
    {
        handle_init_delete(app, false);
        return false;
    }
    // Delete Permanently
    if let Some(keys) = &keyboard.delete_permanently
        && keys.contains(&shortcut)
    {
        handle_init_delete(app, true);
        return false;
    }
    // Task Manager
    if let Some(keys) = &keyboard.task_manager
        && keys.contains(&shortcut)
    {
        app.show_task_manager = !app.show_task_manager;
        return false;
    }

    // Sorting shortcuts
    if let Some(keys) = &keyboard.sort_by_name
        && keys.contains(&shortcut)
    {
        handle_sort(app, crate::app::SortColumn::Name);
        return false;
    }
    if let Some(keys) = &keyboard.sort_by_extension
        && keys.contains(&shortcut)
    {
        handle_sort(app, crate::app::SortColumn::Extension);
        return false;
    }
    if let Some(keys) = &keyboard.sort_by_date
        && keys.contains(&shortcut)
    {
        handle_sort(app, crate::app::SortColumn::Date);
        return false;
    }
    if let Some(keys) = &keyboard.sort_by_size
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
    // Stop input polling
    if let Some(handle) = app.input_polling_handle.take() {
        handle.abort();
    }
    // Pause watcher
    if let Some(watcher) = &mut app.watcher {
        let paths = watcher.watched_paths.clone();
        for path in &paths {
            let _ = watcher.unwatch(path);
        }
    }
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    // Collect current_dir and edited_file_name before any watcher or sync_watcher calls
    let (panel_current_dir, edited_file_name) = {
        let panel = tab_manager.active_tab_mut();
        let dir = panel.current_dir.clone();
        let mut name = None;
        if let Some(entry) = panel.current_entry().cloned()
            && !entry.is_dir
        {
            name = Some(entry.name.clone());
            let file_path = panel.current_dir.join(&entry.name);
            let result =
                tokio::task::spawn_blocking(move || open_in_default_editor(&file_path)).await;
            if let Err(e) = result {
                panel.error = Some(format!("Error opening editor: {}", e));
            } else if let Err(e) = result.unwrap() {
                panel.error = Some(format!("Error opening editor: {}", e));
            }
        }
        (dir, name)
    };
    // Resume watcher
    if let Some(watcher) = &mut app.watcher {
        let _ = watcher.watch(&panel_current_dir);
    }
    app.sync_watcher();
    // Refresh file list and restore cursor
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    }
    .active_tab_mut();
    if let Ok(entries) = crate::fs_ops::list_dir(&panel_current_dir) {
        panel.entries = entries;
        panel.sort_entries();
        if let Some(name) = edited_file_name {
            if let Some(idx) = panel.entries.iter().position(|e| e.name == name) {
                panel.cursor = idx;
            } else if panel.cursor >= panel.entries.len() {
                panel.cursor = panel.entries.len().saturating_sub(1);
            }
        }
    }
    // Restart input polling after editing
    app.input_polling_handle = Some(spawn_input_polling(input_tx.clone()));
    // Force a redraw by sending a synthetic resize event
    let _ = input_tx.send(CrosstermEvent::Resize(0, 0));
    // Redraw UI will be handled by event loop after edit
}

fn handle_task_event(event: crate::tasks::TaskEvent, app: &mut AppState) {
    match event {
        crate::tasks::TaskEvent::Added(_id, _name) => {
            // Optional: notify or log
        }
        crate::tasks::TaskEvent::UpdateStatus(id, status) => {
            app.task_manager.update_task_status(id, status.clone());
            // If failed, maybe show global error?
            if let crate::tasks::TaskStatus::Failed(_e) = status {
                // Log?
            }
        }
        crate::tasks::TaskEvent::UpdateProgress(id, p) => {
            app.task_manager.update_task_progress(id, p);
        }
    }
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

                let progress = (i + 1) as f32 / total as f32;
                let _ = tx.send(crate::tasks::TaskEvent::UpdateProgress(id, progress));
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

    if let Some(entry) = panel.current_entry().cloned()
        && entry.is_dir
    {
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
