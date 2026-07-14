use fm::app::{AppState, PanelSide, TabManager};
use fm::clipboard::InMemoryFileClipboard;
use fm::config::GlobalConfig;
use fm::dir_history::DirectoryHistory;
use fm::fs::fs_local::LocalFs;
use fm::fs::utils::FileEntry;
use fm::handlers::navigation::{
    handle_down, handle_down_search, handle_end, handle_home, handle_page_down, handle_page_up,
    handle_type_char, handle_up, handle_up_search,
};
use fm::handlers::popup_file_filter::{handle_file_filter_event, handle_init_file_filter};
use fm::ssh_history::SshConnectionHistory;
use fm::ssh_manager::SshManager;
use fm::state::FileViewerState;
use fm::tasks::TaskManager;
use std::sync::Arc;
use termina::event::{KeyCode, Modifiers};

fn entry(name: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: if is_dir { None } else { Some(100) },
        modified: None,
        attributes: if is_dir {
            "drwxr-xr-x".to_string()
        } else {
            "-rw-r--r--".to_string()
        },
        selected: false,
    }
}

fn test_app(entries: Vec<FileEntry>) -> AppState {
    let mut tab = fm::app::Tab {
        area: ratatui::layout::Rect::default(),
        provider: Arc::new(LocalFs::new()),
        current_dir: std::path::PathBuf::from("/tmp"),
        entries,
        cursor: 0,
        search: fm::app_state::tabs::IncrementalSearch::default(),
        sort: fm::app_state::tabs::SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        status_msg: None,
        dir_sizes: std::collections::HashMap::new(),
        is_reloading: false,
        file_filter: None,
        file_filter_regex: None,
        visible_indices: Vec::new(),
        visible_set: std::collections::HashSet::new(),
    };
    tab.recompute_visible_indices();
    AppState {
        left: TabManager {
            tabs: vec![tab.clone()],
            active_tab_index: 0,
        },
        right: TabManager {
            tabs: vec![tab],
            active_tab_index: 0,
        },
        active: PanelSide::Left,
        file_viewer: FileViewerState::new(false, "test-theme"),
        fuzzy_search: fm::ui::fuzzy_search_ui::FuzzySearchState::new(),
        popups: fm::app::Popups::new(),
        task_manager: TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
        ssh_manager: Arc::new(SshManager::default()),
        task_decision_txs: std::collections::HashMap::new(),
        show_task_manager: false,
        dir_history: DirectoryHistory::new().unwrap(),
        watcher: None,
        input_polling_handle: None,
        keyboard: fm::config::KeyboardConfig::default(),
        global: GlobalConfig::default(),
        editor_cfg: fm::config::EditorConfig::default(),
        viewer_cfg: fm::config::ViewerConfig::default(),
        ssh_history: SshConnectionHistory::new().unwrap(),
        bookmark_store: fm::bookmarks::BookmarkStore::test_default(),
        clipboard: Box::new(InMemoryFileClipboard::new()),
        remote_watcher: None,
        archive_cache: std::collections::HashMap::new(),
        opener: Arc::new(fm::opener::SystemOpener),
        left_tab_bar_area: ratatui::layout::Rect::default(),
        right_tab_bar_area: ratatui::layout::Rect::default(),
        left_panel_area: ratatui::layout::Rect::default(),
        right_panel_area: ratatui::layout::Rect::default(),
        left_tab_areas: Vec::new(),
        right_tab_areas: Vec::new(),
        last_click: None,
        pending_action: None,
        mouse_button_down_index: None,
    }
}

fn mixed_entries() -> Vec<FileEntry> {
    vec![
        entry("..", true),
        entry("docs", true),
        entry("src", true),
        entry("Cargo.toml", false),
        entry("README.md", false),
        entry("main.rs", false),
        entry("lib.rs", false),
    ]
}

// ---------------------------------------------------------------------------
// Basic filter operations
// ---------------------------------------------------------------------------

#[test]
fn test_set_file_filter_matches_files() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    let tab = app.active_tab();
    assert!(tab.has_file_filter());
    // Directories and ".." are always visible
    assert!(tab.visible_set.contains(&0)); // ".."
    assert!(tab.visible_set.contains(&1)); // docs
    assert!(tab.visible_set.contains(&2)); // src
    // Matching files
    assert!(tab.visible_set.contains(&5)); // main.rs
    assert!(tab.visible_set.contains(&6)); // lib.rs
    // Non-matching files
    assert!(!tab.visible_set.contains(&3)); // Cargo.toml
    assert!(!tab.visible_set.contains(&4)); // README.md
    // visible_indices should have exactly 5 entries (.., docs, src, main.rs, lib.rs)
    assert_eq!(tab.visible_count(), 5);
}

#[test]
fn test_set_file_filter_case_insensitive_regex() {
    let mut app = test_app(mixed_entries());
    // (?i) makes the regex case-insensitive
    app.active_tab_mut()
        .set_file_filter(Some("(?i)README"))
        .unwrap();

    let tab = app.active_tab();
    assert!(tab.visible_set.contains(&4)); // README.md
    // All dirs still visible
    assert!(tab.visible_set.contains(&0)); // ".."
    assert!(tab.visible_set.contains(&1)); // docs
    assert!(tab.visible_set.contains(&2)); // src
    // Non-matching files excluded
    assert!(!tab.visible_set.contains(&3)); // Cargo.toml
    assert!(!tab.visible_set.contains(&5)); // main.rs
    assert!(!tab.visible_set.contains(&6)); // lib.rs
}

#[test]
fn test_clear_file_filter_restores_all() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    assert_eq!(app.active_tab().visible_count(), 5);

    app.active_tab_mut().clear_file_filter();
    let tab = app.active_tab();
    assert!(!tab.has_file_filter());
    // All entries should be visible again
    assert_eq!(tab.visible_count(), 7);
}

#[test]
fn test_invalid_regex_sets_error() {
    let mut app = test_app(mixed_entries());
    let result = app.active_tab_mut().set_file_filter(Some("[invalid"));
    assert!(result.is_err());
    // Filter should not be set
    assert!(!app.active_tab().has_file_filter());
}

#[test]
fn test_empty_pattern_clears_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    assert!(app.active_tab().has_file_filter());

    app.active_tab_mut().set_file_filter(Some("")).unwrap();
    assert!(!app.active_tab().has_file_filter());
    assert_eq!(app.active_tab().visible_count(), 7);
}

#[test]
fn test_none_pattern_clears_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    app.active_tab_mut().set_file_filter(None).unwrap();
    assert!(!app.active_tab().has_file_filter());
}

// ---------------------------------------------------------------------------
// Cursor behavior
// ---------------------------------------------------------------------------

#[test]
fn test_cursor_adjusts_to_visible_on_filter() {
    let mut app = test_app(mixed_entries());
    // Cursor is on "Cargo.toml" (index 3) which will be filtered out
    app.active_tab_mut().cursor = 3;
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    // Cursor should have been moved to a visible entry
    let tab = app.active_tab();
    assert!(tab.visible_set.contains(&tab.cursor));
}

#[test]
fn test_cursor_stays_on_visible_entry() {
    let mut app = test_app(mixed_entries());
    // Cursor is on "src" (index 2, a dir) which stays visible
    app.active_tab_mut().cursor = 2;
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    assert_eq!(app.active_tab().cursor, 2);
}

#[test]
fn test_cursor_adjusts_to_nearest_above() {
    let mut app = test_app(mixed_entries());
    // Entries: 0="..", 1=docs, 2=src, 3=Cargo.toml, 4=README.md, 5=main.rs, 6=lib.rs
    // Cursor on index 4 (README.md) - filtered out.
    // Nearest visible above is index 2 (src).
    app.active_tab_mut().cursor = 4;
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    assert_eq!(app.active_tab().cursor, 2);
}

// ---------------------------------------------------------------------------
// Navigation with filter
// ---------------------------------------------------------------------------

#[test]
fn test_up_filtered_skips_hidden() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    // visible_indices: [0, 1, 2, 5, 6]
    // Cursor on lib.rs (index 6, visible_pos=4)
    app.active_tab_mut().cursor = 6;
    handle_up(&mut app);
    // Should go to main.rs (index 5, visible_pos=3), skipping hidden entries
    assert_eq!(app.active_tab().cursor, 5);
}

#[test]
fn test_down_filtered_skips_hidden() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    // visible_indices: [0, 1, 2, 5, 6]
    // Cursor on src (index 2, visible_pos=2)
    app.active_tab_mut().cursor = 2;
    handle_down(&mut app);
    // Should skip Cargo.toml and README.md, go to main.rs (index 5)
    assert_eq!(app.active_tab().cursor, 5);
}

#[test]
fn test_home_filtered_goes_to_first_visible() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    app.active_tab_mut().cursor = 6;
    handle_home(&mut app);
    // First visible is ".." (index 0)
    assert_eq!(app.active_tab().cursor, 0);
}

#[test]
fn test_end_filtered_goes_to_last_visible() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    app.active_tab_mut().cursor = 0;
    handle_end(&mut app);
    // Last visible is lib.rs (index 6)
    assert_eq!(app.active_tab().cursor, 6);
}

#[test]
fn test_page_up_filtered() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    // visible_indices: [0, 1, 2, 5, 6] (5 entries)
    // Cursor on lib.rs (index 6, visible_pos=4)
    app.active_tab_mut().cursor = 6;
    handle_page_up(&mut app);
    // Page size is 20, but only 5 visible, so should go to first visible
    assert_eq!(app.active_tab().cursor, 0);
}

#[test]
fn test_page_down_filtered() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    // Cursor on ".." (index 0, visible_pos=0)
    app.active_tab_mut().cursor = 0;
    handle_page_down(&mut app);
    // Page size 20 > 5 visible, should clamp to last visible
    assert_eq!(app.active_tab().cursor, 6);
}

// ---------------------------------------------------------------------------
// Select all with filter
// ---------------------------------------------------------------------------

#[test]
fn test_select_all_with_filter_selects_only_visible() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    app.active_tab_mut().select_all();

    let tab = app.active_tab();
    // ".." is never selected
    assert!(!tab.entries[0].selected);
    // Directories are always selected
    assert!(tab.entries[1].selected); // docs
    assert!(tab.entries[2].selected); // src
    // Matching files are selected
    assert!(tab.entries[5].selected); // main.rs
    assert!(tab.entries[6].selected); // lib.rs
    // Non-matching files are NOT selected
    assert!(!tab.entries[3].selected); // Cargo.toml
    assert!(!tab.entries[4].selected); // README.md
}

#[test]
fn test_select_all_without_filter_selects_everything() {
    let mut app = test_app(mixed_entries());
    // No filter set
    app.active_tab_mut().select_all();

    let tab = app.active_tab();
    assert!(!tab.entries[0].selected); // ".." never selected
    assert!(tab.entries[1].selected);
    assert!(tab.entries[2].selected);
    assert!(tab.entries[3].selected);
    assert!(tab.entries[4].selected);
    assert!(tab.entries[5].selected);
    assert!(tab.entries[6].selected);
}

// ---------------------------------------------------------------------------
// Visible counts
// ---------------------------------------------------------------------------

#[test]
fn test_visible_count_matches_indices() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("^C")).unwrap();

    let tab = app.active_tab();
    // Only Cargo.toml matches ^C, plus "..", docs, src are dirs
    assert_eq!(tab.visible_count(), 4); // [.., docs, src, Cargo.toml]
    assert_eq!(tab.visible_indices.len(), 4);
}

#[test]
fn test_visible_file_count_excludes_dirs() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    let tab = app.active_tab();
    // Visible files (non-dir): main.rs, lib.rs = 2
    assert_eq!(tab.visible_file_count(), 2);
}

#[test]
fn test_visible_file_count_no_filter() {
    let app = test_app(mixed_entries());
    let tab = app.active_tab();
    // All files (non-dir, non-..): Cargo.toml, README.md, main.rs, lib.rs = 4
    assert_eq!(tab.visible_file_count(), 4);
}

// ---------------------------------------------------------------------------
// visible_row_to_entry_index
// ---------------------------------------------------------------------------

#[test]
fn test_visible_row_to_entry_index_with_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    app.active_tab_mut().scroll_offset = 0;

    let tab = app.active_tab();
    // Row 0 -> visible_indices[0] = 0 ("..")
    assert_eq!(tab.visible_row_to_entry_index(0), Some(0));
    // Row 1 -> visible_indices[1] = 1 (docs)
    assert_eq!(tab.visible_row_to_entry_index(1), Some(1));
    // Row 3 -> visible_indices[3] = 5 (main.rs)
    assert_eq!(tab.visible_row_to_entry_index(3), Some(5));
    // Row 4 -> visible_indices[4] = 6 (lib.rs)
    assert_eq!(tab.visible_row_to_entry_index(4), Some(6));
    // Out of bounds
    assert_eq!(tab.visible_row_to_entry_index(5), None);
}

#[test]
fn test_visible_row_to_entry_index_without_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().scroll_offset = 0;

    let tab = app.active_tab();
    // Without filter, row maps directly to entry index
    assert_eq!(tab.visible_row_to_entry_index(0), Some(0));
    assert_eq!(tab.visible_row_to_entry_index(3), Some(3));
    assert_eq!(tab.visible_row_to_entry_index(7), None); // out of bounds
}

// ---------------------------------------------------------------------------
// Popup handler
// ---------------------------------------------------------------------------

#[test]
fn test_filter_popup_init_empty() {
    let mut app = test_app(mixed_entries());
    handle_init_file_filter(&mut app);

    assert!(app.popups.file_filter.is_visible);
    assert!(app.popups.file_filter.pattern.is_empty());
    assert_eq!(app.popups.file_filter.cursor_position, 0);
    assert!(app.popups.file_filter.error.is_none());
}

#[test]
fn test_filter_popup_init_with_existing_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    handle_init_file_filter(&mut app);

    assert!(app.popups.file_filter.is_visible);
    assert_eq!(app.popups.file_filter.pattern, "\\.rs$");
    assert_eq!(app.popups.file_filter.cursor_position, 5);
}

#[test]
fn test_filter_popup_enter_applies_filter() {
    let mut app = test_app(mixed_entries());
    handle_init_file_filter(&mut app);

    // Type a regex pattern
    for c in "\\.rs".chars() {
        handle_file_filter_event(KeyCode::Char(c), Modifiers::NONE, &mut app);
    }

    // Press Enter
    handle_file_filter_event(KeyCode::Enter, Modifiers::NONE, &mut app);

    assert!(!app.popups.file_filter.is_visible);
    assert!(app.active_tab().has_file_filter());
    assert_eq!(app.active_tab().visible_count(), 5); // dirs + .rs files
}

#[test]
fn test_filter_popup_escape_cancels() {
    let mut app = test_app(mixed_entries());
    handle_init_file_filter(&mut app);

    // Type something
    handle_file_filter_event(KeyCode::Char('a'), Modifiers::NONE, &mut app);
    assert_eq!(app.popups.file_filter.pattern, "a");

    // Press Escape
    handle_file_filter_event(KeyCode::Escape, Modifiers::NONE, &mut app);

    assert!(!app.popups.file_filter.is_visible);
    assert!(!app.active_tab().has_file_filter());
}

#[test]
fn test_filter_popup_invalid_regex_shows_error() {
    let mut app = test_app(mixed_entries());
    handle_init_file_filter(&mut app);

    // Type an invalid regex
    for c in "[invalid".chars() {
        handle_file_filter_event(KeyCode::Char(c), Modifiers::NONE, &mut app);
    }

    // Press Enter
    handle_file_filter_event(KeyCode::Enter, Modifiers::NONE, &mut app);

    assert!(app.popups.file_filter.is_visible); // popup stays open
    assert!(app.popups.file_filter.error.is_some());
    assert!(!app.active_tab().has_file_filter()); // filter not applied
}

#[test]
fn test_filter_popup_empty_enter_clears_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    assert!(app.active_tab().has_file_filter());

    handle_init_file_filter(&mut app);
    // Pattern is pre-filled, clear it
    app.popups.file_filter.pattern.clear();
    app.popups.file_filter.cursor_position = 0;

    // Press Enter with empty pattern
    handle_file_filter_event(KeyCode::Enter, Modifiers::NONE, &mut app);

    assert!(!app.active_tab().has_file_filter());
    assert_eq!(app.active_tab().visible_count(), 7);
}

#[test]
fn test_filter_popup_typing_clears_error() {
    let mut app = test_app(mixed_entries());
    handle_init_file_filter(&mut app);

    // Type invalid regex and press enter to get error
    for c in "[invalid".chars() {
        handle_file_filter_event(KeyCode::Char(c), Modifiers::NONE, &mut app);
    }
    handle_file_filter_event(KeyCode::Enter, Modifiers::NONE, &mut app);
    assert!(app.popups.file_filter.error.is_some());

    // Type another character — error should clear
    handle_file_filter_event(KeyCode::Char('x'), Modifiers::NONE, &mut app);
    assert!(app.popups.file_filter.error.is_none());
}

// ---------------------------------------------------------------------------
// Search interaction with filter
// ---------------------------------------------------------------------------

#[test]
fn test_search_respects_file_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    // Type "Z" — should NOT match any visible entry (dirs don't contain 'z',
    // and Cargo.toml/README.md are filtered out anyway)
    handle_type_char(&mut app, 'Z');
    let tab = app.active_tab();
    // No visible entry fuzzy-matches "z", so matching_indices should be empty
    assert!(
        tab.search.matching_indices.is_empty(),
        "expected no matches for 'Z', got {:?}",
        tab.search.matching_indices
    );
}

#[test]
fn test_search_finds_only_visible_files() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    // Type "main" — should match main.rs (index 5) via prefix match
    handle_type_char(&mut app, 'm');
    let tab = app.active_tab();
    assert_eq!(tab.search.matching_indices, vec![5]);
    assert_eq!(tab.cursor, 5);
}

#[test]
fn test_search_navigation_with_filter() {
    // Use distinct names so search is unambiguous
    let entries = vec![
        entry("..", true),
        entry("alpha", true),
        entry("beta", true),
        entry("alpha.txt", false),
        entry("beta.txt", false),
        entry("alpha.rs", false),
        entry("beta.rs", false),
    ];
    let mut app = test_app(entries);
    // Filter to only .rs files
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    // visible: [0="..", 1=alpha(dir), 2=beta(dir), 5=alpha.rs, 6=beta.rs]

    // Type "a" — prefix-matches alpha (dir, index 1) and alpha.rs (index 5)
    handle_type_char(&mut app, 'a');
    assert_eq!(app.active_tab().cursor, 1); // first match: alpha (dir)

    // Down search — should go to alpha.rs (index 5)
    handle_down_search(&mut app);
    assert_eq!(app.active_tab().cursor, 5);

    // Down search wraps — back to alpha (index 1)
    handle_down_search(&mut app);
    assert_eq!(app.active_tab().cursor, 1);

    // Up search — wraps to alpha.rs (index 5)
    handle_up_search(&mut app);
    assert_eq!(app.active_tab().cursor, 5);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_filter_allows_only_directories() {
    let mut app = test_app(mixed_entries());
    // Regex that matches nothing
    app.active_tab_mut().set_file_filter(Some("^$")).unwrap();

    let tab = app.active_tab();
    // Only dirs and ".." remain visible
    assert_eq!(tab.visible_count(), 3); // [.., docs, src]
    assert_eq!(tab.visible_file_count(), 0);
}

#[test]
fn test_filter_match_all_files() {
    let mut app = test_app(mixed_entries());
    // .* matches everything
    app.active_tab_mut().set_file_filter(Some(".*")).unwrap();

    let tab = app.active_tab();
    assert_eq!(tab.visible_count(), 7); // all entries
}

#[test]
fn test_filter_on_directory_names() {
    let mut app = test_app(mixed_entries());
    // Filter that matches directory names
    app.active_tab_mut().set_file_filter(Some("^src")).unwrap();

    let tab = app.active_tab();
    // Directories are always visible, but "src" also matches the regex
    assert!(tab.visible_set.contains(&0)); // ".."
    assert!(tab.visible_set.contains(&1)); // docs (dir, always visible)
    assert!(tab.visible_set.contains(&2)); // src (dir + matches)
    // No files match "^src"
    assert!(!tab.visible_set.contains(&3));
    assert!(!tab.visible_set.contains(&4));
    assert!(!tab.visible_set.contains(&5));
    assert!(!tab.visible_set.contains(&6));
    assert_eq!(tab.visible_count(), 3);
}

#[test]
fn test_filter_toggle_on_off() {
    let mut app = test_app(mixed_entries());

    // Apply filter
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();
    assert_eq!(app.active_tab().visible_count(), 5);

    // Clear filter
    app.active_tab_mut().clear_file_filter();
    assert_eq!(app.active_tab().visible_count(), 7);

    // Apply a different filter
    app.active_tab_mut().set_file_filter(Some("Cargo")).unwrap();
    assert_eq!(app.active_tab().visible_count(), 4); // [.., docs, src, Cargo.toml]
}

#[test]
fn test_cursor_visible_pos() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut()
        .set_file_filter(Some("\\.rs$"))
        .unwrap();

    // visible_indices: [0, 1, 2, 5, 6]
    app.active_tab_mut().cursor = 0;
    assert_eq!(app.active_tab().cursor_visible_pos(), Some(0));

    app.active_tab_mut().cursor = 2;
    assert_eq!(app.active_tab().cursor_visible_pos(), Some(2));

    app.active_tab_mut().cursor = 5;
    assert_eq!(app.active_tab().cursor_visible_pos(), Some(3));

    app.active_tab_mut().cursor = 6;
    assert_eq!(app.active_tab().cursor_visible_pos(), Some(4));

    // Cursor on a filtered-out entry
    app.active_tab_mut().cursor = 3;
    assert_eq!(app.active_tab().cursor_visible_pos(), None);
}
