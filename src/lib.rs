pub mod app;
pub mod app_state;
pub mod bookmarks;
pub mod clipboard;
pub mod config;
#[cfg(windows)]
pub mod context_menu;
pub mod dir_history;
pub mod event_loop;
pub mod fs;
pub mod handlers;
pub mod icons;
pub mod large_text;
pub mod layout;
pub mod opener;
pub mod paths;
pub mod ssh_history;
pub mod ssh_known_hosts;
pub mod ssh_manager;
pub mod state;
pub mod tasks;
pub mod test_utils;
pub mod theme;
pub mod ui;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use crate::app::AppState;
use crate::app::PendingAction;
use crate::config::KeyboardConfig;
use crate::config::load_config;
use crate::event_loop::run_event_loop;
use crate::handlers::editor::{execute_open_editor_remote, open_file_in_editor_with_env_handling};
use crate::handlers::terminal::{
    disable_mouse_capture, enable_mouse_capture, execute_toggle_console,
};
use crate::theme::ThemePalette;
use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::TerminaBackend;
use ratatui::termina::{PlatformTerminal, Terminal as _};
use std::env;
use termina::EventReader;

struct InitializedApp {
    app: AppState,
    palette: ThemePalette,
    keyboard: KeyboardConfig,
    #[allow(clippy::struct_field_names)]
    watcher_rx: tokio::sync::mpsc::UnboundedReceiver<crate::fs::watcher::WatcherEvent>,
    #[allow(clippy::struct_field_names)]
    task_rx: tokio::sync::mpsc::UnboundedReceiver<crate::tasks::UiEvent>,
    #[allow(clippy::struct_field_names)]
    image_load_rx: tokio::sync::mpsc::UnboundedReceiver<crate::state::ImageLoadResult>,
    #[allow(clippy::struct_field_names)]
    content_load_rx: tokio::sync::mpsc::UnboundedReceiver<crate::state::ContentLoadResult>,
    #[allow(clippy::struct_field_names)]
    highlight_rx: tokio::sync::mpsc::UnboundedReceiver<crate::state::HighlightBatch>,
}

async fn initialize_app() -> Result<InitializedApp> {
    let (keyboard, global_config, editor_cfg, viewer_cfg, ssh_cfg) =
        load_config().map_err(anyhow::Error::msg)?;

    let theme_name = global_config
        .theme
        .as_deref()
        .unwrap_or(crate::theme::DEFAULT_THEME_NAME);
    let palette = crate::theme::get_theme(theme_name).unwrap_or_else(crate::theme::default_theme);

    let cwd = env::current_dir()?;

    let dir_history = crate::dir_history::DirectoryHistory::new().map_err(anyhow::Error::msg)?;

    let (watcher_tx, watcher_rx) = tokio::sync::mpsc::unbounded_channel();
    let watcher = fs::watcher::AppWatcher::new(&watcher_tx).ok();
    let watcher = if let Some(mut w) = watcher {
        let _ = w.watch(&cwd);
        Some(w)
    } else {
        None
    };

    let (task_tx, task_rx) = tokio::sync::mpsc::unbounded_channel();
    let task_manager = crate::tasks::TaskManager::new(task_tx.clone());

    let persistent_state = crate::app::AppState::load_state().ok().flatten();

    let (image_load_tx, image_load_rx) = tokio::sync::mpsc::unbounded_channel();
    let (content_load_tx, content_load_rx) = tokio::sync::mpsc::unbounded_channel();
    let (highlight_tx, highlight_rx) = tokio::sync::mpsc::unbounded_channel();
    let bookmark_store = crate::bookmarks::BookmarkStore::new()?;

    let ctx = crate::app::AppConfigContext {
        palette: &palette,
        keyboard: keyboard.clone(),
        global: global_config,
        editor_cfg,
        viewer_cfg,
        ssh_cfg,
        dir_history,
        watcher: watcher.map(|w| Box::new(w) as Box<dyn fs::watcher::FileSystemWatcher>),
        remote_watcher: Some(Box::new(fs::watcher::RemoteWatcher::new(&watcher_tx))
            as Box<dyn fs::watcher::FileSystemWatcher>),
        task_manager,
        bookmark_store,
    };

    let mut app = if let Some(state) = persistent_state {
        let left = crate::app_state::tabs::TabManager::from_persistent(state.left).await?;
        let right = crate::app_state::tabs::TabManager::from_persistent(state.right).await?;

        crate::app::AppState::new(left, right, state.active_side, ctx)
    } else {
        crate::app::AppState::new(
            crate::app_state::tabs::TabManager::new(&cwd).await?,
            crate::app_state::tabs::TabManager::new(&cwd).await?,
            crate::app_state::tabs::PanelSide::Left,
            ctx,
        )
    };

    app.file_viewer.image.set_image_load_channel(image_load_tx);
    app.file_viewer
        .text
        .set_content_load_channel(content_load_tx);
    app.file_viewer
        .text
        .set_highlight_worker(crate::state::HighlightWorker::start(highlight_tx));

    let context_key = app.active_tab().provider.context_key();
    app.dir_history.record_visit(&context_key.to_string(), &cwd);

    app.sync_watcher();

    Ok(InitializedApp {
        app,
        palette,
        keyboard,
        watcher_rx,
        task_rx,
        image_load_rx,
        content_load_rx,
        highlight_rx,
    })
}

fn setup_terminal() -> Result<(Terminal<TerminaBackend<PlatformTerminal>>, EventReader)> {
    let mut output = PlatformTerminal::new()?;
    output.enter_raw_mode()?;
    let reader = output.event_reader();
    let terminal = Terminal::new(TerminaBackend::new(output))?;
    Ok((terminal, reader))
}

/// Runs the main application.
///
/// # Errors
///
/// Returns an error if the application fails to initialize or run.
pub async fn run() -> Result<()> {
    // Clean up remote-editing temp files left behind by dead processes
    crate::handlers::editor::sweep_stale_temp_files();

    let InitializedApp {
        mut app,
        palette,
        keyboard,
        mut watcher_rx,
        mut task_rx,
        mut image_load_rx,
        mut content_load_rx,
        mut highlight_rx,
    } = initialize_app().await?;

    app.file_viewer.init_picker().await;
    let (mut terminal, mut reader) = setup_terminal()?;

    if app.global.mouse.unwrap_or(true) {
        enable_mouse_capture()?;
    }

    loop {
        // 2. Run Event Loop
        let result = run_event_loop(
            &mut terminal,
            &mut app,
            &palette,
            keyboard.clone(),
            event_loop::EventSources {
                reader: reader.clone(),
                watcher_rx: &mut watcher_rx,
                task_rx: &mut task_rx,
                image_load_rx: &mut image_load_rx,
                content_load_rx: &mut content_load_rx,
                highlight_rx: &mut highlight_rx,
            },
        )
        .await;

        // Check if event loop encountered an error
        let action = match result {
            Ok(act) => act,
            Err(e) => {
                let _ = app.dir_history.save();
                let _ = app.save_state();
                app.cleanup_sensitive_data();
                return Err(e);
            }
        };

        // 3. Handle Pending Action
        match action {
            Some(PendingAction::OpenEditorLocal(path, name)) => {
                if let Err(e) = open_file_in_editor_with_env_handling(&mut app, &path, name).await {
                    app.active_tab_mut().error = Some(e.to_string());
                }
            }
            Some(PendingAction::OpenEditorRemote {
                temp_path,
                remote_path,
                provider,
                original_checksum,
            }) => {
                if let Err(e) = execute_open_editor_remote(
                    &mut app,
                    temp_path,
                    remote_path,
                    provider,
                    original_checksum,
                )
                .await
                {
                    app.active_tab_mut().error = Some(e.to_string());
                }
            }
            Some(PendingAction::ToggleConsole) => {
                drop(terminal);

                if let Err(e) = execute_toggle_console(&mut app).await {
                    app.active_tab_mut().error = Some(e.to_string());
                }

                let (t, r) = setup_terminal()?;
                terminal = t;
                reader = r;

                if app.global.mouse.unwrap_or(true) {
                    enable_mouse_capture()?;
                }
            }
            Some(PendingAction::WindowsContextMenu(path)) => {
                #[cfg(windows)]
                if let Err(e) = crate::context_menu::show_context_menu(&path) {
                    app.active_tab_mut().error = Some(e.to_string());
                }
                #[cfg(not(windows))]
                let _ = path;
            }
            None => {
                break;
            }
        }
    }

    if app.global.mouse.unwrap_or(true) {
        disable_mouse_capture()?;
    }

    drop(terminal);

    let _ = app.dir_history.save();
    let _ = app.save_state();
    app.cleanup_sensitive_data();

    Ok(())
}
