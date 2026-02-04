pub mod app;
pub mod app_state;
pub mod clipboard;
pub mod config;
pub mod conflict_ui;
pub mod copy_move_ui;
pub mod create_dir_ui;
pub mod create_file_ui;
pub mod delete_ui;
pub mod dir_history;
pub mod drive_select_ui;
pub mod empty_trash_ui;
pub mod error_ui;
pub mod event_loop;
pub mod fs;
pub mod fs_local;
// pub mod fs_ops; // Removed, now fs::ops
pub mod fs_provider;
pub mod fs_rsync;
pub mod fs_sftp;
pub mod fuzzy_search_ui;
pub mod handlers;
pub mod help_ui;
pub mod icons;
pub mod quit_ui;
pub mod remote_edit_ui;
pub mod rename_ui;
pub mod ssh_history;
pub mod ssh_manager;
pub mod ssh_ui;
pub mod state;
pub mod task_ui;
pub mod tasks;
pub mod theme;
pub mod ui;
pub mod ui_utils;
pub mod watcher;

use crate::config::load_config;
use crate::event_loop::run_event_loop;
use anyhow::Result;
use crossterm::terminal::enable_raw_mode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::env;

pub async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let (keyboard, global_config, editor_cfg, viewer_cfg, ssh_cfg) =
        load_config().map_err(anyhow::Error::msg)?;

    let theme_name = global_config
        .theme
        .as_deref()
        .unwrap_or("catppuccin macchiato");
    let palette = crate::theme::get_theme(theme_name).unwrap_or_else(crate::theme::default_theme);
    terminal.clear()?;

    let cwd = env::current_dir()?;

    let dir_history = crate::dir_history::DirectoryHistory::new().map_err(anyhow::Error::msg)?;

    let (watcher_tx, mut watcher_rx) = tokio::sync::mpsc::unbounded_channel();
    let watcher = crate::watcher::AppWatcher::new(watcher_tx.clone()).ok();
    let watcher = if let Some(mut w) = watcher {
        let _ = w.watch(&cwd);
        Some(w)
    } else {
        None
    };

    let (task_tx, mut task_rx) = tokio::sync::mpsc::unbounded_channel();
    let task_manager = crate::tasks::TaskManager::new(task_tx.clone());

    let persistent_state = crate::app::AppState::load_state().ok().flatten();

    let (image_load_tx, mut image_load_rx) = tokio::sync::mpsc::unbounded_channel();

    let ctx = crate::app::AppConfigContext {
        palette: &palette,
        global: global_config,
        editor_cfg,
        viewer_cfg,
        ssh_cfg,
        dir_history,
        watcher: watcher.map(|w| Box::new(w) as Box<dyn crate::watcher::FileSystemWatcher>),
        remote_watcher: Some(
            Box::new(crate::watcher::RemoteWatcher::new(watcher_tx.clone()))
                as Box<dyn crate::watcher::FileSystemWatcher>,
        ),
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

    app.file_viewer.image_load_tx = Some(image_load_tx);
    app.file_viewer.init_picker();

    let context_key = app.active_tab().provider.context_key();
    app.dir_history.record_visit(&context_key, &cwd);

    app.sync_watcher();

    run_event_loop(
        &mut terminal,
        &mut app,
        &palette,
        keyboard,
        &mut watcher_rx,
        &mut task_rx,
        &mut image_load_rx,
    )
    .await?;

    let _ = app.dir_history.save();

    let _ = app.save_state();

    app.cleanup_sensitive_data();

    Ok(())
}
