mod fs_ops;
mod ui;
mod app;
mod event_loop;

use anyhow::Result;
use catppuccin::PALETTE;
use std::env;
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use crate::app::{AppState, PanelState, PanelSide};
use crate::fs_ops::{list_dir};

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
        typed_buffer: String::new(),
        last_type_time: None,
    };
    let right_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        selected: 0,
        history: vec![cwd.clone()],
        history_index: 0,
        error: None,
        typed_buffer: String::new(),
        last_type_time: None,
    };

    let mut app = AppState {
        left: left_panel,
        right: right_panel,
        active: PanelSide::Left,
    };

    event_loop::run_event_loop(&mut terminal, &mut app, palette)?;
    disable_raw_mode()?;
    Ok(())
}

