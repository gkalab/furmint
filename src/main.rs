mod app;
mod config;
mod event_loop;
mod fs_ops;
mod theme;
mod ui;

use crate::app::{AppState, PanelSide, PanelState};
use crate::config::load_config;
use crate::fs_ops::list_dir;
use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::env;

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;

    // Load config
    let (keyboard, theme_config) = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            disable_raw_mode()?;
            std::process::exit(1);
        }
    };

    // Select theme
    let theme_name = theme_config
        .name
        .as_deref()
        .unwrap_or("catppuccin macchiato");
    let palette = theme::get_theme(theme_name).unwrap_or_else(theme::default_theme);
    terminal.clear()?;

    let cwd = env::current_dir()?;
    let left_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        cursor: 0,
        history: vec![app::HistoryEntry {
            path: cwd.clone(),
            cursor: 0,
        }],
        history_index: 0,
        error: None,
        typed_buffer: String::new(),
        last_type_time: None,
    };
    let right_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        cursor: 0,
        history: vec![app::HistoryEntry {
            path: cwd.clone(),
            cursor: 0,
        }],
        history_index: 0,
        error: None,
        typed_buffer: String::new(),
        last_type_time: None,
    };

    let mut app = AppState {
        left: left_panel,
        right: right_panel,
        active: PanelSide::Left,
        file_viewer: crate::app::FileViewerState::new(palette.is_dark, theme_name),
    };

    event_loop::run_event_loop(&mut terminal, &mut app, &palette, keyboard)?;
    disable_raw_mode()?;
    Ok(())
}
