use crate::app::AppState;
use crate::config::KeyboardConfig;
use crate::handlers::navigation::reset_expired_search;
use crate::handlers::popup_misc::handle_task_event;
use crate::handlers::terminal::{disable_mouse_capture, enable_mouse_capture};
use crate::theme::ThemePalette;
use ratatui::Terminal;
use termina::EventReader;
use termina::event::Event;
use tokio::sync::mpsc::UnboundedReceiver;

#[must_use]
pub fn spawn_input_polling(
    input_tx: tokio::sync::mpsc::UnboundedSender<Event>,
    reader: EventReader,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if input_tx.is_closed() {
                break;
            }
            match tokio::task::spawn_blocking({
                let reader = reader.clone();
                move || reader.poll(Some(std::time::Duration::from_millis(100)), |_| true)
            })
            .await
            {
                Ok(Ok(true)) => {
                    let reader = reader.clone();
                    match tokio::task::spawn_blocking(move || reader.read(|_| true)).await {
                        Ok(Ok(ev)) => {
                            if input_tx.send(ev).is_err() {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                Ok(Ok(false)) => {}
                Ok(Err(_)) | Err(_) => break,
            }
        }
    })
}

/// Input event sources for the event loop.
pub(crate) struct EventSources<'a> {
    pub reader: EventReader,
    pub watcher_rx: &'a mut UnboundedReceiver<crate::fs::watcher::WatcherEvent>,
    pub task_rx: &'a mut UnboundedReceiver<crate::tasks::TaskEvent>,
    pub image_load_rx: &'a mut UnboundedReceiver<crate::state::ImageLoadResult>,
}

/// Runs the main event loop for the application.
///
/// # Errors
///
/// Returns an error if the event loop encounters an unrecoverable error.
pub(crate) async fn run_event_loop<B>(
    terminal: &mut Terminal<B>,
    app: &mut AppState,
    palette: &ThemePalette,
    keyboard: KeyboardConfig,
    sources: EventSources<'_>,
) -> anyhow::Result<Option<crate::app::PendingAction>>
where
    B: ratatui::backend::Backend,
    B::Error: Send + Sync + 'static,
{
    // Create channel for terminal events
    let (input_tx, mut input_rx) = tokio::sync::mpsc::unbounded_channel();

    // Start input polling and store handle
    app.input_polling_handle = Some(spawn_input_polling(
        input_tx.clone(),
        sources.reader.clone(),
    ));

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

    let mut should_exit = false;
    let mut mouse_capture_active = app.global.mouse.unwrap_or(true);

    // Initial draw
    draw_ui(terminal, app, palette, &keyboard, &mut mouse_capture_active)?;

    while !should_exit {
        // Automatically resume input polling if it was taken by a handler
        if app.input_polling_handle.is_none() {
            app.input_polling_handle = Some(spawn_input_polling(
                input_tx.clone(),
                sources.reader.clone(),
            ));
        }

        // Explicit redraw if requested (e.g. after editor or console toggle)
        if app.needs_redraw {
            terminal.clear()?;
            draw_ui(terminal, app, palette, &keyboard, &mut mouse_capture_active)?;
            app.needs_redraw = false;
        }
        tokio::select! {
                            // Handle watcher events
                            Some(event) = sources.watcher_rx.recv() => {
                                handle_watcher_event(event, app);
                                app.sync_watcher();
                                draw_ui(terminal, app, palette, &keyboard, &mut mouse_capture_active)?;
                            }
                            // Handle input events
                            Some(event) = input_rx.recv() => {
                let mut exit = handle_event(event, app, &keyboard).await;
                // Drain any other immediately available events to prevent buffering
                // This allows skipping frames if input is faster than rendering
        while !exit {
            match input_rx.try_recv() {
                Ok(ev) => {
                    if handle_event(ev, app, &keyboard).await {
                        exit = true;
                    }
                }
                Err(_) => break,
            }
        }

                                if exit {
                                    should_exit = true;
                                } else {
                                    if app.needs_redraw {
                                        terminal.clear()?;
                                        app.needs_redraw = false;
                                    }
                                    draw_ui(terminal, app, palette, &keyboard, &mut mouse_capture_active)?;
                                }

                                // Sync watcher if navigation happened
                                app.sync_watcher();
                            }
                            // Handle task events
                            Some(event) = sources.task_rx.recv() => {
                                handle_task_event(event, app);
                                draw_ui(terminal, app, palette, &keyboard, &mut mouse_capture_active)?;
                            }
                            _ = interval.tick() => {
                                app.task_manager.cleanup_tasks();
                                app.cleanup_archive_cache();
                                // Reset search if timeout has expired
                                reset_expired_search(app);
                                // Poll watchers
                                if let Some(w) = &mut app.watcher {
                                    let _ = w.poll();
                                }
                                if let Some(w) = &mut app.remote_watcher {
                                    let _ = w.poll();
                                }
                                draw_ui(terminal, app, palette, &keyboard, &mut mouse_capture_active)?;
                            }
                            // Handle image resize requests immediately and off-thread
                            Some(request) = async {
                                if let Some(rx) = &mut app.file_viewer.resize_rx {
                                    rx.recv().await
                                } else {
                                    std::future::pending().await
                                }
                            } => {
                                let encoded = tokio::task::spawn_blocking(move || -> Result<ratatui_image::thread::ResizeResponse, _> {
                                    request.resize_encode()
                                }).await.ok().and_then(std::result::Result::ok);

                                if let (Some(encoded), Some(protocol)) = (encoded, &mut app.file_viewer.protocol) {
                                    let _ = protocol.update_resized_protocol(encoded);
                                    draw_ui(terminal, app, palette, &keyboard, &mut mouse_capture_active)?;
                                }
                            }
                            // Handle image load results
                            Some(load_result) = sources.image_load_rx.recv() => {
                                app.file_viewer.handle_load_result(load_result);
                                draw_ui(terminal, app, palette, &keyboard, &mut mouse_capture_active)?;
                            }
                            else => break,
                        }
        if let Some(action) = app.pending_action.take() {
            return Ok(Some(action));
        }
    }
    terminal.clear()?;
    Ok(None)
}

fn handle_watcher_event(event: crate::fs::watcher::WatcherEvent, app: &mut AppState) {
    match event {
        crate::fs::watcher::WatcherEvent::FileSystemChange(paths) => {
            let handle_tab = |tab: &mut crate::app::Tab| {
                // Watcher only supports local filesystem
                if !tab.provider.is_local() {
                    return;
                }

                // Check info about current directory
                let current_exists = tab.current_dir.exists();

                if !current_exists {
                    // Directory removed, try to go up
                    let _ = tab.go_up();
                }

                // Check if we need to reload
                let needs_reload = paths
                    .iter()
                    .any(|p| p == &tab.current_dir || p.parent() == Some(&tab.current_dir));

                if needs_reload && let Ok(entries) = tab.provider.list_dir(&tab.current_dir) {
                    tab.reload_preserving_state(entries);
                }
            };

            for tab in &mut app.left.tabs {
                handle_tab(tab);
            }
            for tab in &mut app.right.tabs {
                handle_tab(tab);
            }
        }
        crate::fs::watcher::WatcherEvent::RemoteReloadRequested => {
            app.reload_remote();
        }
        crate::fs::watcher::WatcherEvent::Error(_) => {}
    }
}

fn draw_ui<B>(
    terminal: &mut Terminal<B>,
    app: &mut AppState,
    palette: &ThemePalette,
    keyboard: &KeyboardConfig,
    mouse_capture_active: &mut bool,
) -> anyhow::Result<()>
where
    B: ratatui::backend::Backend,
    B::Error: Send + Sync + 'static,
{
    let should_mouse_be_active = app.global.mouse.unwrap_or(true);
    if should_mouse_be_active != *mouse_capture_active {
        if should_mouse_be_active {
            let _ = enable_mouse_capture();
        } else {
            let _ = disable_mouse_capture();
        }
        *mouse_capture_active = should_mouse_be_active;
    }

    terminal.draw(|f| {
        crate::ui::main_ui::draw_main_layout(f, app, palette);
        crate::ui::main_ui::draw_all_popups(f, app, palette, keyboard);
    })?;
    Ok(())
}

/// Returns true if the event is a quit event (Ctrl-q or Esc)
pub async fn handle_event(ev: Event, app: &mut AppState, keyboard: &KeyboardConfig) -> bool {
    crate::handlers::main_handler::route_event(ev, app, keyboard).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::input_utils::keyevent_to_string;
    use termina::event::{KeyCode, Modifiers};

    #[test]
    fn test_keyevent_to_string() {
        assert_eq!(
            keyevent_to_string(KeyCode::Function(3), Modifiers::CONTROL),
            "Ctrl-F3"
        );
        assert_eq!(
            keyevent_to_string(KeyCode::Char('p'), Modifiers::CONTROL),
            "Ctrl-p"
        );
        assert_eq!(
            keyevent_to_string(KeyCode::Left, Modifiers::ALT),
            "Alt-Left"
        );
    }

    #[test]
    fn test_handle_watcher_event_filesystem_change() {
        // Setup AppState mock: two tabs, stub current_dir, fake entries
        use crate::fs::watcher::WatcherEvent;
        let mut app = crate::app::AppState::test_default();
        app.left.active_tab_mut().current_dir = std::path::PathBuf::from("/mock");
        app.left.active_tab_mut().entries = vec![crate::fs::utils::FileEntry {
            name: "testfile.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(12),
            modified: None,
            attributes: String::new(),
            selected: false,
        }];
        app.right.active_tab_mut().current_dir = std::path::PathBuf::from("/mock");
        let paths = vec![std::path::PathBuf::from("/mock")];
        let event = WatcherEvent::FileSystemChange(paths);
        super::handle_watcher_event(event, &mut app);
        // Check cursor and error remain valid
        assert!(app.left.active_tab().cursor == 0);
        assert!(app.left.active_tab().error.is_none());
    }

    #[test]
    fn test_handle_watcher_event_preserves_selection() {
        use crate::fs::utils::FileEntry;
        use crate::fs::watcher::WatcherEvent;
        let mut app = crate::app::AppState::test_default();

        // Note: we need to mock list_dir or ensure it returns what we expect.
        // In this test, handle_watcher_event will call list_dir("/mock").
        // Since "/mock" doesn't exist, list_dir will fail, and it won't reload entries.
        // Wait, I need a real directory to test reload.

        let tmp_dir = tempfile::tempdir().unwrap();
        let file_path = tmp_dir.path().join("testfile.txt");
        std::fs::File::create(&file_path).unwrap();

        app.left.active_tab_mut().current_dir = tmp_dir.path().to_path_buf();
        app.left.active_tab_mut().entries = vec![FileEntry {
            name: "testfile.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(0),
            modified: None,
            attributes: String::new(),
            selected: true,
        }];

        let paths = vec![tmp_dir.path().to_path_buf()];
        let event = WatcherEvent::FileSystemChange(paths);

        super::handle_watcher_event(event, &mut app);

        // Check if selection is preserved
        assert!(
            app.left
                .active_tab()
                .entries
                .iter()
                .any(|e| e.name == "testfile.txt" && e.selected),
            "Selection should be preserved after watcher reload"
        );
    }

    #[test]
    fn test_handle_watcher_event_error() {
        use crate::fs::watcher::WatcherEvent;
        let mut app = crate::app::AppState::test_default();
        let event = WatcherEvent::Error("test error".to_string());
        super::handle_watcher_event(event, &mut app);
        // Should not panic or change error field
        assert!(app.left.active_tab().error.is_none());
    }

    #[tokio::test]
    async fn test_handle_insert_moves_cursor_down() {
        use crate::fs::utils::FileEntry;
        use termina::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, Modifiers};
        let mut app = crate::app::AppState::test_default();
        app.left.active_tab_mut().current_dir = std::path::PathBuf::from("/mock");
        app.left.active_tab_mut().entries = vec![
            FileEntry {
                name: "file1.txt".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(10),
                modified: None,
                attributes: String::new(),
                selected: false,
            },
            FileEntry {
                name: "file2.txt".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(10),
                modified: None,
                attributes: String::new(),
                selected: false,
            },
        ];
        app.right.active_tab_mut().current_dir = std::path::PathBuf::from("/mock");

        let keyboard = KeyboardConfig::default();
        let (_input_tx, _) = tokio::sync::mpsc::unbounded_channel::<Event>();

        // Initial state: cursor at 0, file1 not selected
        assert_eq!(app.left.active_tab().cursor, 0);
        assert!(!app.left.active_tab().entries[0].selected);

        handle_event(
            Event::Key(KeyEvent {
                code: KeyCode::Insert,
                modifiers: Modifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            }),
            &mut app,
            &keyboard,
        )
        .await;

        // After Insert: file1 should be selected, cursor should be at 1
        assert!(app.left.active_tab().entries[0].selected);
        assert_eq!(app.left.active_tab().cursor, 1);
    }
}
