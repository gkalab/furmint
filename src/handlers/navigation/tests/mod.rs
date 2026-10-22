//! Test suite for `handlers::navigation`.
mod file_filter;

mod enter;
mod movement;
mod search;
mod viewer;

use crate::app::AppState;
use crate::app_state::tabs::{PanelSide, TabManager};
use crate::fs::utils::FileEntry;
use crate::handlers::navigation::{
    handle_directory_up, handle_down, handle_down_search, handle_end, handle_enter_directory,
    handle_home, handle_open_archive, handle_open_item, handle_page_down, handle_page_up,
    handle_sort, handle_tab, handle_toggle_selection, handle_type_char, handle_up,
    handle_up_search, reset_expired_search, reset_search, update_viewer_content,
};

fn test_app(entries: Vec<FileEntry>) -> AppState {
    let mut tab = crate::test_utils::create_test_tab();
    tab.entries = entries;
    tab.current_dir = std::path::PathBuf::from("/tmp");
    let left_tm = TabManager {
        tabs: vec![tab.clone()],
        active_tab_index: 0,
    };
    let right_tm = TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    };
    crate::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(right_tm)
        .build()
}
