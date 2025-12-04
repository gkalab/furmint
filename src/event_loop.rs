use ratatui::prelude::*;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crate::app::{AppState, PanelSide};
use catppuccin::Flavor;
use crate::fs_ops::list_dir;
use crate::ui::{draw_panel, draw_panel_status};
use crate::config::KeyboardConfig;

pub fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut AppState,
    palette: &Flavor,
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

fn draw_ui(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, app: &AppState, palette: &Flavor) -> anyhow::Result<()> {
    terminal.draw(|f| {
        let size = f.area();
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1), // panels
                Constraint::Length(1), // status lines
            ])
            .split(size);
        let panel_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(vertical_chunks[0]);
        let status_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(vertical_chunks[1]);
        draw_panel(f, &app.left, app.active == PanelSide::Left, panel_chunks[0], palette);
        draw_panel(f, &app.right, app.active == PanelSide::Right, panel_chunks[1], palette);
        draw_panel_status(f, &app.left, status_chunks[0], palette, app.active == PanelSide::Left);
        draw_panel_status(f, &app.right, status_chunks[1], palette, app.active == PanelSide::Right);
    })?;
    Ok(())
}

fn poll_events() -> anyhow::Result<Vec<Event>> {
    let mut events = Vec::new();
    while event::poll(std::time::Duration::from_millis(10))? {
        events.push(event::read()?);
    }
    Ok(events)
}

/// Returns true if the event is a quit event (Ctrl-q or Esc)
pub fn handle_event(ev: Event, app: &mut AppState, keyboard: &KeyboardConfig) -> bool {
    match ev {
        Event::Key(KeyEvent { code, modifiers, .. }) => {
            if (code == KeyCode::Char('q') && modifiers == KeyModifiers::CONTROL) || code == KeyCode::Esc {
                return true;
            }
            let shortcut = keyevent_to_string(code, modifiers);
            // Previous directory
            if let Some(keys) = &keyboard.previous_directory {
                if keys.contains(&shortcut) {
                    handle_ctrl_left(app);
                    return false;
                }
            }
            // Next directory
            if let Some(keys) = &keyboard.next_directory {
                if keys.contains(&shortcut) {
                    handle_ctrl_right(app);
                    return false;
                }
            }
            match (code, modifiers) {
                (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                    handle_type_char(app, c);
                }
                _ => {
                    // Clear buffer for any non-letter key
                    let panel = match app.active {
                        PanelSide::Left => &mut app.left,
                        PanelSide::Right => &mut app.right,
                    };
                    panel.typed_buffer.clear();
                    panel.last_type_time = None;
                    match (code, modifiers) {
                        (KeyCode::Tab, _) => handle_tab(app),
                        (KeyCode::Up, _) => handle_up(app),
                        (KeyCode::Down, _) => handle_down(app),
                        (KeyCode::PageUp, _) => handle_page_up(app),
                        (KeyCode::PageDown, _) => handle_page_down(app),
                        (KeyCode::Home, _) => handle_home(app),
                        (KeyCode::End, _) => handle_end(app),
                        (KeyCode::Enter, _) => handle_enter(app),
                        (KeyCode::Backspace, _) => handle_backspace(app),
                        _ => {}
                    }
                }
            }
        }
        Event::Resize(_, _) => {}
        _ => {}
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
        KeyCode::Char(c) => c.to_string(),
        _ => String::new(),
    };
    parts.push(key);
    parts.join("-")
}

fn handle_tab(app: &mut AppState) {
    app.active = match app.active {
        PanelSide::Left => PanelSide::Right,
        PanelSide::Right => PanelSide::Left,
    };
}

fn handle_up(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    if panel.selected > 0 {
        panel.selected -= 1;
    }
}

fn handle_down(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    if panel.selected + 1 < panel.entries.len() {
        panel.selected += 1;
    }
}

fn handle_page_up(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let visible_rows = 20; // fallback, will be recalculated in draw_panel
    if panel.selected >= visible_rows {
        panel.selected -= visible_rows;
    } else {
        panel.selected = 0;
    }
}

fn handle_page_down(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let visible_rows = 20; // fallback, will be recalculated in draw_panel
    let max_idx = panel.entries.len().saturating_sub(1);
    if panel.selected + visible_rows <= max_idx {
        panel.selected += visible_rows;
    } else {
        panel.selected = max_idx;
    }
}

fn handle_home(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    panel.selected = 0;
}

fn handle_end(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    if !panel.entries.is_empty() {
        panel.selected = panel.entries.len() - 1;
    }
}

fn handle_enter(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    // Save current selection to history
    if panel.history_index < panel.history.len() {
        panel.history[panel.history_index].selected = panel.selected;
    }
    let entry = &panel.entries[panel.selected];
    if entry.is_dir {
        let mut new_dir = panel.current_dir.clone();
        if entry.name == ".." {
            if let Some(parent) = panel.current_dir.parent() {
                new_dir = parent.to_path_buf();
            }
        } else {
            new_dir.push(&entry.name);
        }
        match list_dir(&new_dir) {
            Ok(entries) => {
                panel.current_dir = new_dir.clone();
                panel.entries = entries;
                // Restore selection if new_dir is in history
                if let Some((idx, hist)) = panel.history.iter().enumerate().find(|(_, h)| h.path == new_dir) {
                    panel.selected = hist.selected.min(panel.entries.len().saturating_sub(1));
                    panel.history_index = idx;
                } else {
                    panel.selected = 0;
                    // Update history
                    if panel.history_index + 1 < panel.history.len() {
                        panel.history.truncate(panel.history_index + 1);
                    }
                    panel.history.push(crate::app::HistoryEntry { path: new_dir.clone(), selected: 0 });
                    panel.history_index += 1;
                }
                panel.error = None;
            }
            Err(e) => {
                panel.error = Some(format!("Error: {}", e));
            }
        }
    }
}

fn handle_backspace(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    // Save current selection to history
    if panel.history_index < panel.history.len() {
        panel.history[panel.history_index].selected = panel.selected;
    }
    if let Some(parent) = panel.current_dir.parent() {
        let parent = parent.to_path_buf();
        match list_dir(&parent) {
            Ok(entries) => {
                panel.current_dir = parent.clone();
                panel.entries = entries;
                // Restore selection if parent is in history
                if let Some((idx, hist)) = panel.history.iter().enumerate().find(|(_, h)| h.path == parent) {
                    panel.selected = hist.selected.min(panel.entries.len().saturating_sub(1));
                    panel.history_index = idx;
                } else {
                    panel.selected = 0;
                    if panel.history_index + 1 < panel.history.len() {
                        panel.history.truncate(panel.history_index + 1);
                    }
                    panel.history.push(crate::app::HistoryEntry { path: parent.clone(), selected: 0 });
                    panel.history_index += 1;
                }
                panel.error = None;
            }
            Err(e) => {
                panel.error = Some(format!("Error: {}", e));
            }
        }
    }
}

fn handle_ctrl_left(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    // Save current selection to history
    if panel.history_index < panel.history.len() {
        panel.history[panel.history_index].selected = panel.selected;
    }
    if panel.history_index > 0 {
        panel.history_index -= 1;
        let hist = &panel.history[panel.history_index];
        match list_dir(&hist.path) {
            Ok(entries) => {
                panel.current_dir = hist.path.clone();
                panel.entries = entries;
                panel.selected = hist.selected.min(panel.entries.len().saturating_sub(1));
                panel.error = None;
            }
            Err(e) => {
                panel.error = Some(format!("Error: {}", e));
            }
        }
    }
}

fn handle_ctrl_right(app: &mut AppState) {
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    // Save current selection to history
    if panel.history_index < panel.history.len() {
        panel.history[panel.history_index].selected = panel.selected;
    }
    if panel.history_index + 1 < panel.history.len() {
        panel.history_index += 1;
        let hist = &panel.history[panel.history_index];
        match list_dir(&hist.path) {
            Ok(entries) => {
                panel.current_dir = hist.path.clone();
                panel.entries = entries;
                panel.selected = hist.selected.min(panel.entries.len().saturating_sub(1));
                panel.error = None;
            }
            Err(e) => {
                panel.error = Some(format!("Error: {}", e));
            }
        }
    }
}

fn handle_type_char(app: &mut AppState, c: char) {
    use std::time::Instant;
    let panel = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let now = Instant::now();
    let reset_threshold = std::time::Duration::from_secs(1);
    // If last_type_time is None or too old, reset buffer
    if panel.last_type_time.map_or(true, |t| now.duration_since(t) > reset_threshold) {
        panel.typed_buffer.clear();
    }
    panel.typed_buffer.push(c);
    panel.last_type_time = Some(now);
    let typed = panel.typed_buffer.to_lowercase();
    // Find first entry whose name starts with typed
    if let Some((idx, _)) = panel.entries.iter().enumerate()
        .find(|(_, entry)| entry.name.to_lowercase().starts_with(&typed)) {
        panel.selected = idx;
    }
}
