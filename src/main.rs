mod app;
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
mod fs_ops;
mod fuzzy_search_ui;
mod help_ui;
mod quit_ui;
mod rename_ui;
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
    let bin_name = args.get(0).map(|s| s.as_str()).unwrap_or("fm");

    // Handle CLI arguments
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        println!("fm - A TUI file manager");
        println!();
        println!("Usage: {} [OPTIONS]", bin_name);
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
                println!("Default configuration created at: {:?}", path);
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error creating default config: {}", e);
                std::process::exit(1);
            }
        }
    }

    enable_raw_mode()?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;

    // Load config
    let (keyboard, global_config, editor_cfg, viewer_cfg) = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            disable_raw_mode()?;
            std::process::exit(1);
        }
    };

    // Select theme
    let theme_name = global_config
        .theme
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

    let persistent_state = crate::app::AppState::load_state().ok().flatten();

    let mut app = if let Some(state) = persistent_state {
        let left = crate::app::TabManager::from_persistent(state.left)?;
        let right = crate::app::TabManager::from_persistent(state.right)?;

        crate::app::AppState {
            left,
            right,
            active: state.active_side,
            file_viewer: crate::app::FileViewerState::new(
                palette.is_dark,
                global_config.theme.as_deref().unwrap_or("default"),
            ),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            rename_popup: crate::app::RenameState::new(),
            create_directory_popup: crate::app::CreateDirectoryState::new(),
            delete_popup: crate::app::DeleteState::new(),
            empty_trash_popup: crate::app::EmptyTrashState::new(),
            copy_move_popup: crate::app::CopyMoveState::new(),
            conflict_popup: crate::app::ConflictState::new(),
            error_popup: crate::app::ErrorState::new(),
            quit_confirmation: crate::app::QuitConfirmationState::new(),
            help_popup: crate::app::HelpState::new(),
            drive_select_popup: crate::app::DriveSelectState::new(),
            task_manager,
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history,
            watcher,
            input_polling_handle: None,
            needs_redraw: false,
            global: global_config,
            editor_cfg,
            viewer_cfg,
            create_file_popup: crate::app::CreateFileState::new(),
        }
    } else {
        crate::app::AppState {
            create_file_popup: crate::app::CreateFileState::new(),
            left: crate::app::TabManager::new(cwd.clone())?,
            right: crate::app::TabManager::new(cwd.clone())?,
            active: crate::app::PanelSide::Left,
            file_viewer: crate::app::FileViewerState::new(
                palette.is_dark,
                global_config.theme.as_deref().unwrap_or("default"),
            ),
            fuzzy_search: crate::fuzzy_search_ui::FuzzySearchState::new(),
            rename_popup: crate::app::RenameState::new(),
            create_directory_popup: crate::app::CreateDirectoryState::new(),
            delete_popup: crate::app::DeleteState::new(),
            empty_trash_popup: crate::app::EmptyTrashState::new(),
            copy_move_popup: crate::app::CopyMoveState::new(),
            conflict_popup: crate::app::ConflictState::new(),
            error_popup: crate::app::ErrorState::new(),
            quit_confirmation: crate::app::QuitConfirmationState::new(),
            help_popup: crate::app::HelpState::new(),
            drive_select_popup: crate::app::DriveSelectState::new(),
            task_manager,
            task_decision_txs: std::collections::HashMap::new(),
            show_task_manager: false,
            dir_history,
            watcher,
            input_polling_handle: None,
            needs_redraw: false,
            global: global_config,
            editor_cfg,
            viewer_cfg,
        }
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

    // Save app state on exit
    if let Err(e) = app.save_state() {
        eprintln!("Error saving app state: {e}");
    }

    disable_raw_mode()?;
    Ok(())
}
