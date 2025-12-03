mod fs_ops;
mod ui;
mod app;

use anyhow::Result;
use catppuccin::PALETTE;
use std::env;
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use crate::app::{AppState, PanelState, PanelSide};
use crate::fs_ops::{list_dir};
use crate::ui::{draw_panel, draw_panel_status};
use ratatui::prelude::*;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    let palette = &PALETTE.macchiato;
    terminal.clear()?;

    let cwd = env::current_dir()?;
    let left_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        selected: 0,
        history: vec![cwd.clone()],
        history_index: 0,
        error: None,
    };
    let right_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        selected: 0,
        history: vec![cwd.clone()],
        history_index: 0,
        error: None,
    };
    let mut app = AppState {
        left: left_panel,
        right: right_panel,
        active: PanelSide::Left,

    };

    let mut should_exit = false;
    while !should_exit {
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
            draw_panel(f, &app.left, app.active == PanelSide::Left, panel_chunks[0], &palette);
            draw_panel(f, &app.right, app.active == PanelSide::Right, panel_chunks[1], &palette);
            draw_panel_status(f, &app.left, status_chunks[0], &palette, app.active == PanelSide::Left);
            draw_panel_status(f, &app.right, status_chunks[1], &palette, app.active == PanelSide::Right);
        })?;

        while event::poll(std::time::Duration::from_millis(10))? {
            match event::read()? {
                Event::Key(KeyEvent { code, modifiers, .. }) => {
                    // Quit on Ctrl-q or Esc
                    if (code == KeyCode::Char('q') && modifiers == KeyModifiers::CONTROL) || code == KeyCode::Esc {
                        should_exit = true;
                        break;
                    }
                    match (code, modifiers) {
                        (KeyCode::Tab, _) => {
                            app.active = match app.active {
                                PanelSide::Left => PanelSide::Right,
                                PanelSide::Right => PanelSide::Left,
                            };
                        }
                        (KeyCode::Up, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if panel.selected > 0 {
                                panel.selected -= 1;
                            }
                        }
                        (KeyCode::Down, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if panel.selected + 1 < panel.entries.len() {
                                panel.selected += 1;
                            }
                        }
                        (KeyCode::PageUp, _) => {
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
                        (KeyCode::PageDown, _) => {
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
                        (KeyCode::Home, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            panel.selected = 0;
                        }
                        (KeyCode::End, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if !panel.entries.is_empty() {
                                panel.selected = panel.entries.len() - 1;
                            }
                        }
                        (KeyCode::Enter, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
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
                                        panel.selected = 0;
                                        // Update history
                                        if panel.history_index + 1 < panel.history.len() {
                                            panel.history.truncate(panel.history_index + 1);
                                        }
                                        panel.history.push(new_dir);
                                        panel.history_index += 1;
                                        panel.error = None;
                                    }
                                    Err(e) => {
                                        panel.error = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        (KeyCode::Backspace, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if let Some(parent) = panel.current_dir.parent() {
                                let parent = parent.to_path_buf();
                                match list_dir(&parent) {
                                    Ok(entries) => {
                                        panel.current_dir = parent.clone();
                                        panel.entries = entries;
                                        panel.selected = 0;
                                        if panel.history_index + 1 < panel.history.len() {
                                            panel.history.truncate(panel.history_index + 1);
                                        }
                                        panel.history.push(parent);
                                        panel.history_index += 1;
                                        panel.error = None;
                                    }
                                    Err(e) => {
                                        panel.error = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        (KeyCode::Left, KeyModifiers::CONTROL) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if panel.history_index > 0 {
                                panel.history_index -= 1;
                                let dir = panel.history[panel.history_index].clone();
                                match list_dir(&dir) {
                                    Ok(entries) => {
                                        panel.current_dir = dir;
                                        panel.entries = entries;
                                        panel.selected = 0;
                                        panel.error = None;
                                    }
                                    Err(e) => {
                                        panel.error = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        (KeyCode::Right, KeyModifiers::CONTROL) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if panel.history_index + 1 < panel.history.len() {
                                panel.history_index += 1;
                                let dir = panel.history[panel.history_index].clone();
                                match list_dir(&dir) {
                                    Ok(entries) => {
                                        panel.current_dir = dir;
                                        panel.entries = entries;
                                        panel.selected = 0;
                                        panel.error = None;
                                    }
                                    Err(e) => {
                                        panel.error = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        (KeyCode::Char('q'), _) => {
                            // Do nothing, handled above
                        }
                        _ => {}
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    disable_raw_mode()?;
    Ok(())
}

