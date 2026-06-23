use fm::app::AppState;
use fm::clipboard::InMemoryFileClipboard;
use fm::handlers::popup_conflict::handle_conflict_event;
use fm::ssh_history::SshConnectionHistory;
use fm::ssh_manager::SshManager;
use fm::tasks::TaskDecision;
use std::collections::HashMap;
use std::path::Path;
use termina::event::KeyCode;
use tokio::sync::mpsc;

fn app_with_conflict(task_id: usize) -> (AppState, mpsc::Receiver<TaskDecision>) {
    use fm::state::*;
    use fm::tasks::TaskEvent;
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();
    let (dec_tx, dec_rx) = mpsc::channel(1);
    let mut app = AppState {
        left: fm::app::TabManager::new(Path::new("/tmp")).unwrap(),
        right: fm::app::TabManager::new(Path::new("/tmp")).unwrap(),
        active: fm::app::PanelSide::Left,
        file_viewer: FileViewerState::new(false, ""),
        fuzzy_search: fm::ui::fuzzy_search_ui::FuzzySearchState::new(),
        popups: fm::app::Popups::new(),
        task_manager: fm::tasks::TaskManager::new(task_tx),
        task_decision_txs: {
            let mut map = HashMap::new();
            map.insert(task_id, dec_tx);
            map
        },
        show_task_manager: false,
        dir_history: fm::dir_history::DirectoryHistory::new().unwrap(),
        watcher: None,
        input_polling_handle: None,
        needs_redraw: false,
        keyboard: fm::config::KeyboardConfig::default(),
        global: fm::config::GlobalConfig {
            mouse: Some(false),
            ..fm::config::GlobalConfig::default()
        },
        editor_cfg: fm::config::EditorConfig::default(),
        viewer_cfg: fm::config::ViewerConfig::default(),
        ssh_history: SshConnectionHistory::new().unwrap(),
        clipboard: Box::new(InMemoryFileClipboard::new()),
        remote_watcher: None,
        archive_cache: std::collections::HashMap::new(),
        ssh_manager: std::sync::Arc::new(SshManager::default()),
        opener: std::sync::Arc::new(fm::opener::SystemOpener),
        left_tab_bar_area: ratatui::layout::Rect::default(),
        right_tab_bar_area: ratatui::layout::Rect::default(),
        left_panel_area: ratatui::layout::Rect::default(),
        right_panel_area: ratatui::layout::Rect::default(),
        left_tab_areas: Vec::new(),
        right_tab_areas: Vec::new(),
        last_click: None,
        pending_action: None,
        mouse_button_down_index: None,
        bookmark_store: fm::bookmarks::BookmarkStore::test_default(),
    };

    app.popups.conflict.is_visible = true;
    app.popups.conflict.task_id = task_id;
    (app, dec_rx)
}

#[tokio::test]
async fn ignores_irrelevant_keys() {
    let (mut app, mut rx) = app_with_conflict(42);
    let _ = handle_conflict_event(KeyCode::Char('z'), &mut app).await;
    assert!(
        app.popups.conflict.is_visible,
        "Unmapped key should not reset popup"
    );
    assert!(
        rx.try_recv().is_err(),
        "No decision should be sent for unmapped key"
    );
}
