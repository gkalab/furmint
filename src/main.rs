mod app;
mod config;
mod delete_ui;
mod dir_history;
mod event_loop;
mod fs_ops;
mod fuzzy_search_ui;
mod rename_ui;
mod task_ui;
mod tasks;
mod theme;
mod ui;
mod ui_utils;
mod watcher;

use crate::config::load_config;
use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
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

    // Initialize watcher
    let (watcher_tx, mut watcher_rx) = tokio::sync::mpsc::unbounded_channel();
    let watcher = crate::watcher::AppWatcher::new(watcher_tx)
        .ok()
        .map(|mut w| {
            if let Err(e) = w.watch(&cwd) {
                eprintln!("Failed to start watcher: {}", e);
            }
            w
        });

    // Initialize task manager channel
    let (task_tx, mut task_rx) = tokio::sync::mpsc::unbounded_channel();
    let task_manager = crate::tasks::TaskManager::new(task_tx);

    let mut app = app::AppState {
        left: crate::app::TabManager::new(cwd.clone())?,
        right: crate::app::TabManager::new(cwd.clone())?,
        active: crate::app::PanelSide::Left,
        file_viewer: crate::app::FileViewerState::new(
            palette.is_dark,
            theme_config.name.as_deref().unwrap_or("default"),
        ),
        fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
        rename_popup: crate::app::RenameState::new(),
        delete_popup: crate::app::DeleteState::new(),
        task_manager,
        show_task_manager: false,
        dir_history,
        watcher,
        input_polling_handle: None,
        needs_redraw: false,
    };

    // Record initial directory visit
    app.dir_history.record_visit(&cwd);

    // Initial sync of watcher
    app.sync_watcher();

    event_loop::run_event_loop(
        &mut terminal,
        &mut app,
        &palette,
        keyboard,
        &mut watcher_rx,
        &mut task_rx,
    )
    .await?;

    // Save directory history on exit
    if let Err(e) = app.dir_history.save() {
        eprintln!("Error saving directory history: {e}");
    }

    disable_raw_mode()?;
    Ok(())
}
