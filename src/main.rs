mod app;
pub mod app_state;
mod config;
mod conflict_ui;
mod copy_move_ui;
mod create_dir_ui;
mod create_file_ui;
mod delete_ui;
mod dir_history;
mod drive_select_ui;
mod empty_trash_ui;
mod error_ui;
mod event_loop;
pub mod fs_local;
mod fs_ops;
pub mod fs_provider;
mod fuzzy_search_ui;
mod handlers;
mod help_ui;
mod quit_ui;
mod rename_ui;
mod state;
mod task_ui;
mod tasks;
mod theme;
mod ui;
mod ui_utils;
mod watcher;

use crate::config::{create_default_config, load_config};
use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let bin_name = args.first().map_or("fm", std::string::String::as_str);

    // Handle CLI arguments
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        println!("fm - A TUI file manager");
        println!();
        println!("Usage: {bin_name} [OPTIONS]");
        println!();
        println!("Options:");
        println!("  -h, --help            Show this help message");
        println!("  --version             Show version information");
        println!("  --create-config       Create default configuration file");
        println!();
        println!("Key bindings (default):");
        println!("  F1                  Show help screen");
        println!("  Ctrl-q              Quit");
        println!("  Arrow keys          Navigate");
        println!("  Enter               Enter directory / Open file");
        println!("  Tab                 Change panel");
        return Ok(());
    }
    if args.contains(&"--version".to_string()) {
        println!("fm version {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.contains(&"--create-config".to_string()) {
        match create_default_config() {
            Ok(path) => {
                println!("Default configuration created at: {}", path.display());
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error creating default config: {e}");
                std::process::exit(1);
            }
        }
    }

    let result = run().await;

    disable_raw_mode().ok();

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    Ok(())
}

async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;

    // Load config
    let (keyboard, global_config, editor_cfg, viewer_cfg) =
        load_config().map_err(anyhow::Error::msg)?;

    // Select theme
    let theme_name = global_config
        .theme
        .as_deref()
        .unwrap_or("catppuccin macchiato");
    let palette = theme::get_theme(theme_name).unwrap_or_else(theme::default_theme);
    terminal.clear()?;

    let cwd = env::current_dir()?;

    // Initialize directory history
    let dir_history = dir_history::DirectoryHistory::new().map_err(anyhow::Error::msg)?;

    // Initialize watcher
    let (watcher_tx, mut watcher_rx) = tokio::sync::mpsc::unbounded_channel();
    let watcher = crate::watcher::AppWatcher::new(watcher_tx).ok();
    let watcher = if let Some(mut w) = watcher {
        let _ = w.watch(&cwd);
        Some(w)
    } else {
        None
    };

    // Initialize task manager channel
    let (task_tx, mut task_rx) = tokio::sync::mpsc::unbounded_channel();
    let task_manager = crate::tasks::TaskManager::new(task_tx);

    let persistent_state = crate::app::AppState::load_state().ok().flatten();

    let ctx = crate::app::AppConfigContext {
        palette: &palette,
        global: global_config,
        editor_cfg,
        viewer_cfg,
        dir_history,
        watcher,
        task_manager,
    };

    let mut app = if let Some(state) = persistent_state {
        let left = crate::app::TabManager::from_persistent(state.left)?;
        let right = crate::app::TabManager::from_persistent(state.right)?;

        crate::app::AppState::new(left, right, state.active_side, ctx)
    } else {
        crate::app::AppState::new(
            crate::app::TabManager::new(&cwd)?,
            crate::app::TabManager::new(&cwd)?,
            crate::app::PanelSide::Left,
            ctx,
        )
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
    let _ = app.dir_history.save();

    // Save app state on exit
    let _ = app.save_state();

    Ok(())
}
