//! Test suite for `handlers::popup_copy_move`.
mod archive;
mod clipboard;
mod copy_move;
mod validate;

use crate::app::AppState;
use crate::app_state::tabs::{PanelSide, Tab, TabManager};
use crate::clipboard::{FileClipboardAction, FileClipboardData};
use crate::fs::local::LocalFs;
use crate::fs::provider::FileSystemProvider;
use crate::fs::utils::FileEntry;
use crate::handlers::popup_copy_move::{
    handle_copy_move_event, handle_init_copy, handle_init_move, handle_paste,
};
use crate::state::CopyMoveAction;
use crate::ui::text_edit::TextEditState;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use termina::event::{KeyCode, Modifiers};

fn make_fileentry(name: &str, selected: bool, is_dir: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected,
    }
}

fn make_tab(path: &str, entries: Vec<FileEntry>, cursor: usize) -> Tab {
    let current_dir = PathBuf::from(path);
    let mut tab = Tab {
        provider: Arc::new(LocalFs::new()),
        current_dir: current_dir.clone(),
        entries,
        cursor,
        search: crate::app_state::tabs::IncrementalSearch::default(),
        sort: crate::app_state::tabs::SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        ssh_session_id: None,
        status_msg: None,
        dir_sizes: HashMap::new(),
        is_reloading: false,
        visible_indices: Vec::new(),
        filter: crate::state::FileFilterState::new(),
        show_hidden: true,
    };
    // Real tabs always carry a populated visible set; `is_visible` reads it.
    tab.recompute_visible_indices();
    tab.cursor = cursor;
    tab
}

fn make_tab_manager(tab: Tab) -> TabManager {
    TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    }
}

fn minimal_state_with_entries(
    active: PanelSide,
    left_entries: Vec<FileEntry>,
    right_entries: Vec<FileEntry>,
    left_cursor: usize,
    right_cursor: usize,
) -> AppState {
    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(make_tab_manager(make_tab(
            "/left",
            left_entries,
            left_cursor,
        )))
        .right(make_tab_manager(make_tab(
            "/right",
            right_entries,
            right_cursor,
        )))
        .build();
    app.panels.active = active;
    app
}

/// Poll for a file to appear (the copy task runs on a spawned tokio task).
async fn wait_for_file(path: &std::path::Path, deadline_secs: u64) -> bool {
    let start = std::time::Instant::now();
    while !path.exists() {
        if start.elapsed() > std::time::Duration::from_secs(deadline_secs) {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    true
}
