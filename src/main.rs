mod app;
mod config;
mod dir_history;
mod event_loop;
mod fs_ops;
mod fuzzy_search_ui;
mod theme;
mod ui;
mod ui_utils;

use crate::app::{AppState, PanelSide};
use crate::config::load_config;
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

    // Initialize directory history
    let dir_history = match dir_history::DirectoryHistory::new() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error loading directory history: {e}");
            disable_raw_mode()?;
            std::process::exit(1);
        }
    };

    let mut app = AppState {
        left: app::TabManager::new(cwd.clone())?,
        right: app::TabManager::new(cwd.clone())?,
        active: PanelSide::Left,
        file_viewer: crate::app::FileViewerState::new(palette.is_dark, theme_name),
        fuzzy_search: fuzzy_search_ui::FuzzySearchState::new(),
        dir_history,
    };

    // Record initial directory visit
    app.dir_history.record_visit(&cwd);

    event_loop::run_event_loop(&mut terminal, &mut app, &palette, keyboard)?;

    // Save directory history on exit
    if let Err(e) = app.dir_history.save() {
        eprintln!("Error saving directory history: {e}");
    }

    disable_raw_mode()?;
    Ok(())
}
