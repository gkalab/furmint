mod fs_ops;
mod ui;
mod app;
mod event_loop;
mod config;

use anyhow::Result;
use catppuccin::PALETTE;
use std::env;
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use crate::app::{AppState, PanelState, PanelSide};
use crate::fs_ops::{list_dir};
use crate::config::load_config;

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;

    // Load config
    let (keyboard, theme) = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            disable_raw_mode()?;
            std::process::exit(1);
        }
    };

    // Select theme
    let palette = match theme.name.as_deref() {
        Some("catppuccin macchiato") | None => &PALETTE.macchiato,
        Some("catppuccin latte") => &PALETTE.latte,
        Some("catppuccin frappe") => &PALETTE.frappe,
        Some("catppuccin mocha") => &PALETTE.mocha,
        Some(_) => &PALETTE.macchiato, // fallback
    };
    terminal.clear()?;

    let cwd = env::current_dir()?;
    let left_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        cursor: 0,
        history: vec![app::HistoryEntry { path: cwd.clone(), cursor: 0 }],
        history_index: 0,
        error: None,
        typed_buffer: String::new(),
        last_type_time: None,
    };
    let right_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        cursor: 0,
        history: vec![app::HistoryEntry { path: cwd.clone(), cursor: 0 }],
        history_index: 0,
        error: None,
        typed_buffer: String::new(),
        last_type_time: None,
    };

    // Determine if theme is dark (latte is light, others are dark)
    let is_dark_theme = !matches!(theme.name.as_deref(), Some("catppuccin latte"));

    let mut app = AppState {
        left: left_panel,
        right: right_panel,
        active: PanelSide::Left,
        file_viewer: crate::app::FileViewerState::new(is_dark_theme),
    };

    event_loop::run_event_loop(&mut terminal, &mut app, palette, keyboard)?;
    disable_raw_mode()?;
    Ok(())
}

