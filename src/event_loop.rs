use crate::app::{AppState, PanelSide};
use crate::config::KeyboardConfig;
use crate::theme::ThemePalette;
use crate::ui::{draw_panel, draw_panel_status};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::prelude::*;
use std::env;
use std::process::Command;

pub fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut AppState,
    palette: &ThemePalette,
    keyboard: KeyboardConfig,
) -> anyhow::Result<()> {
    let mut should_exit = false;
    while !should_exit {
        draw_ui(terminal, app, palette)?;
        let events = poll_events()?;
        for event in events {
            if handle_event(event, app, &keyboard) {
                should_exit = true;
                break;
            }
        }
    }
    terminal.clear()?;
    Ok(())
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
    })?;
    Ok(())
}

fn poll_events() -> anyhow::Result<Vec<Event>> {
    let mut events = Vec::new();

    // Drain all available events to prevent buffering
    // This is especially important for rapid key presses (like scrolling)
    while event::poll(std::time::Duration::from_millis(0))? {
        events.push(event::read()?);
    }

    // If no events are immediately available, wait a short time for one
    if events.is_empty() && event::poll(std::time::Duration::from_millis(10))? {
        events.push(event::read()?);
    }

    Ok(events)
}

/// Returns true if the event is a quit event (Ctrl-q or Esc)
pub fn handle_event(ev: Event, app: &mut AppState, keyboard: &KeyboardConfig) -> bool {
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
                    && !app.rename_popup.is_visible)
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

            handle_main_panel_event(code, modifiers, app, keyboard);
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

fn handle_main_panel_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut AppState,
    keyboard: &KeyboardConfig,
) -> bool {
    let shortcut = keyevent_to_string(code, modifiers);

    // Tab management shortcuts
    // New tab
    if let Some(keys) = &keyboard.new_tab {
        if keys.contains(&shortcut) {
            handle_new_tab(app);
            return false;
        }
    }
    // Next tab
    if let Some(keys) = &keyboard.next_tab {
        if keys.contains(&shortcut) {
            handle_next_tab(app);
            return false;
        }
    }
    // Previous tab
    if let Some(keys) = &keyboard.prev_tab {
        if keys.contains(&shortcut) {
            handle_prev_tab(app);
            return false;
        }
    }
    // Close tab
    if let Some(keys) = &keyboard.close_tab {
        if keys.contains(&shortcut) {
            handle_close_tab(app);
            return false;
        }
    }

    // Previous directory from history
    if let Some(keys) = &keyboard.history_previous {
        if keys.contains(&shortcut) {
            handle_history_previous(app);
            return false;
        }
    }
    // Next directory from history
    if let Some(keys) = &keyboard.history_next {
        if keys.contains(&shortcut) {
            handle_history_next(app);
            return false;
        }
    }
    // Enter directory
    if let Some(keys) = &keyboard.enter_directory {
        if keys.contains(&shortcut) {
            handle_enter_directory(app);
            return false;
        }
    }
    // Directory up
    if let Some(keys) = &keyboard.directory_up {
        if keys.contains(&shortcut) {
            handle_directory_up(app);
            return false;
        }
    }
    // Edit
    if let Some(keys) = &keyboard.edit {
        if keys.contains(&shortcut) {
            handle_edit(app);
            return false;
        }
    }
    // Fuzzy search
    if let Some(keys) = &keyboard.fuzzy_search {
        if keys.contains(&shortcut) {
            app.fuzzy_search.is_visible = true;
            app.fuzzy_search.reset();
            // Initialize with all directories sorted by score
            let results = app.dir_history.fuzzy_search("");
            app.fuzzy_search.filtered_dirs = results.into_iter().map(|(p, _)| p).collect();
            app.fuzzy_search.selected_index = 0;
            return false;
        }
    }

    // Rename
    if let Some(keys) = &keyboard.rename {
        if keys.contains(&shortcut) {
            handle_init_rename(app);
            return false;
        }
    }

    // Sorting shortcuts
    if let Some(keys) = &keyboard.sort_by_name {
        if keys.contains(&shortcut) {
            handle_sort(app, crate::app::SortColumn::Name);
            return false;
        }
    }
    if let Some(keys) = &keyboard.sort_by_extension {
        if keys.contains(&shortcut) {
            handle_sort(app, crate::app::SortColumn::Extension);
            return false;
        }
    }
    if let Some(keys) = &keyboard.sort_by_date {
        if keys.contains(&shortcut) {
            handle_sort(app, crate::app::SortColumn::Date);
            return false;
        }
    }
    if let Some(keys) = &keyboard.sort_by_size {
        if keys.contains(&shortcut) {
            handle_sort(app, crate::app::SortColumn::Size);
            return false;
        }
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

fn handle_edit(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let panel = tab_manager.active_tab_mut();
    if let Some(entry) = panel.current_entry().cloned() {
        if !entry.is_dir {
            let file_path = panel.current_dir.join(&entry.name);
            // Check if file is text (not binary)
            match crate::fs_ops::read_file_content(&file_path, 8192) {
                Ok(content) => {
                    if content.contains("Binary file detected") {
                        panel.error = Some("Cannot open binary file in editor".to_string());
                    } else {
                        // Launch editor
                        match open_in_default_editor(&file_path) {
                            Ok(_) => {}
                            Err(e) => panel.error = Some(format!("Editor error: {}", e)),
                        }
                    }
                }
                Err(e) => {
                    panel.error = Some(format!("Error reading file: {}", e));
                }
            }
        }
    }
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
        .map_or(true, |t| now.duration_since(t) > reset_threshold)
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
            app.rename_popup.new_name.insert(app.rename_popup.cursor_position, c);
            app.rename_popup.cursor_position += 1;
        }
        KeyCode::Backspace => {
            app.rename_popup.error = None; // Clear error on typing
            if app.rename_popup.cursor_position > 0 {
                app.rename_popup.new_name.remove(app.rename_popup.cursor_position - 1);
                app.rename_popup.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if app.rename_popup.cursor_position < app.rename_popup.new_name.len() {
                app.rename_popup.new_name.remove(app.rename_popup.cursor_position);
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
    let old_path = app.rename_popup.parent_dir.join(&app.rename_popup.original_name);
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
                    if let Some(idx) = panel.entries.iter().position(|e| e.name == app.rename_popup.new_name) {
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
