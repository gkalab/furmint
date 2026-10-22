use crate::app::AppState;
use crate::app_state::tabs::TabManager;
use crate::fs::utils::FileEntry;

mod navigation;

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
    let mut tab = crate::test_utils::create_test_tab();
    tab.entries = entries;
    tab.current_dir = std::path::PathBuf::from("/tmp");
    tab.recompute_visible_indices();
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

#[tokio::test]
async fn test_set_file_filter_matches_files() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    let tab = app.active_tab();
    assert!(tab.filter.is_active());
    // Directories are never filtered out
    assert!(tab.is_visible(0)); // ".."
    assert!(tab.is_visible(1)); // docs
    assert!(tab.is_visible(2)); // src
    // Matching files are visible
    assert!(tab.is_visible(5)); // main.rs
    assert!(tab.is_visible(6)); // lib.rs
    // Non-matching files are hidden
    assert!(!tab.is_visible(3)); // Cargo.toml
    assert!(!tab.is_visible(4)); // README.md
    // visible_indices should be exactly [.., docs, src, main.rs, lib.rs]
    assert_eq!(tab.visible_count(), 5);
}

#[tokio::test]
async fn test_set_file_filter_never_hides_directories() {
    let mut app = test_app(mixed_entries());
    // A pattern that matches no file at all
    app.active_tab_mut().set_file_filter(Some("zzz*")).unwrap();

    let tab = app.active_tab();
    // All directories stay visible, no files do
    assert_eq!(tab.visible_indices, vec![0, 1, 2]);
    assert_eq!(tab.visible_file_count(), 0);
}

#[tokio::test]
async fn test_set_file_filter_case_sensitivity() {
    let mut app = test_app(mixed_entries());
    // Differs from README.md only in case
    app.active_tab_mut()
        .set_file_filter(Some("readme*"))
        .unwrap();

    let tab = app.active_tab();
    // Case handling follows the platform filesystem: case-insensitive on
    // Windows and macOS, case-sensitive on Linux.
    #[cfg(any(windows, target_os = "macos"))]
    {
        assert!(tab.is_visible(4)); // README.md
        assert_eq!(tab.visible_count(), 4); // [.., docs, src, README.md]
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        assert!(!tab.is_visible(4)); // README.md
        assert_eq!(tab.visible_count(), 3); // [.., docs, src]
    }
    // Directories are always visible, Cargo.toml never matches
    assert!(tab.is_visible(1)); // docs
    assert!(!tab.is_visible(3)); // Cargo.toml
}

#[tokio::test]
async fn test_clear_file_filter_restores_all() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    assert_eq!(app.active_tab().visible_count(), 5);

    app.active_tab_mut().clear_file_filter();
    let tab = app.active_tab();
    assert!(!tab.filter.is_active());
    // All entries should be visible again
    assert_eq!(tab.visible_count(), 7);
}

#[tokio::test]
async fn test_invalid_glob_sets_error() {
    let mut app = test_app(mixed_entries());
    let result = app.active_tab_mut().set_file_filter(Some("[invalid"));
    assert!(result.is_err());
    // Filter should not be set
    assert!(!app.active_tab().filter.is_active());
}

#[tokio::test]
async fn test_empty_pattern_clears_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    assert!(app.active_tab().filter.is_active());

    app.active_tab_mut().set_file_filter(Some("")).unwrap();
    assert!(!app.active_tab().filter.is_active());
    assert_eq!(app.active_tab().visible_count(), 7);
}

#[tokio::test]
async fn test_none_pattern_clears_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    app.active_tab_mut().set_file_filter(None).unwrap();
    assert!(!app.active_tab().filter.is_active());
}

// ---------------------------------------------------------------------------
// Cursor behavior
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cursor_adjusts_to_visible_on_filter() {
    let mut app = test_app(mixed_entries());
    // Cursor is on "Cargo.toml" (row 3 while everything is shown) which will be
    // filtered out
    app.active_tab_mut().cursor = 3;
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    // Cursor should have been moved onto a visible entry
    let tab = app.active_tab();
    assert!(tab.is_visible(tab.cursor_entry_index().unwrap()));
}

#[tokio::test]
async fn test_cursor_stays_on_visible_entry() {
    let mut app = test_app(mixed_entries());
    // Cursor is on main.rs (entry 5, row 5 with no filter), which matches the
    // filter
    app.active_tab_mut().cursor = 5;
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    assert_eq!(
        app.active_tab().current_entry().map(|e| e.name.as_str()),
        Some("main.rs")
    );
}

#[tokio::test]
async fn test_cursor_adjusts_to_nearest_above() {
    let mut app = test_app(mixed_entries());
    // Entries: 0="..", 1=docs, 2=src, 3=Cargo.toml, 4=README.md, 5=main.rs, 6=lib.rs
    // Cursor on README.md (row 4) - filtered out.
    // Nearest visible entry above is "src" (entry 2).
    app.active_tab_mut().cursor = 4;
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    let tab = app.active_tab();
    assert_eq!(tab.current_entry().map(|e| e.name.as_str()), Some("src"));
    assert_eq!(tab.cursor, 2); // visible row of "src": [.., docs, src, main.rs, lib.rs]
}

// ---------------------------------------------------------------------------
// Select all with filter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_select_all_with_filter_selects_only_visible() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    app.active_tab_mut().select_all();

    let tab = app.active_tab();
    // ".." is never selected
    assert!(!tab.entries[0].selected); // ".."
    // Directories are always visible and get selected
    assert!(tab.entries[1].selected); // docs
    assert!(tab.entries[2].selected); // src
    // Non-matching files are NOT visible, so NOT selected
    assert!(!tab.entries[3].selected); // Cargo.toml
    assert!(!tab.entries[4].selected); // README.md
    // Matching files are selected
    assert!(tab.entries[5].selected); // main.rs
    assert!(tab.entries[6].selected); // lib.rs
}

#[tokio::test]
async fn test_select_all_without_filter_selects_everything() {
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

#[tokio::test]
async fn test_visible_count_matches_indices() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("toml")).unwrap();

    let tab = app.active_tab();
    // The bare pattern is implicitly wrapped (*toml*) and matches only
    // Cargo.toml; the directories are always visible
    assert_eq!(tab.visible_count(), 4); // [.., docs, src, Cargo.toml]
    assert_eq!(tab.visible_indices.len(), 4);
}

#[tokio::test]
async fn test_visible_file_count_excludes_dirs() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    let tab = app.active_tab();
    // Visible files (non-dir): main.rs, lib.rs = 2
    assert_eq!(tab.visible_file_count(), 2);
}

#[tokio::test]
async fn test_visible_file_count_no_filter() {
    let app = test_app(mixed_entries());
    let tab = app.active_tab();
    // All files (non-dir, non-..): Cargo.toml, README.md, main.rs, lib.rs = 4
    assert_eq!(tab.visible_file_count(), 4);
}

// ---------------------------------------------------------------------------
// visible_row_to_entry_index
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_visible_row_to_entry_index_with_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    app.active_tab_mut().scroll_offset = 0;

    let tab = app.active_tab();
    // Row 0 -> visible_indices[0] = 0 ("..")
    assert_eq!(tab.visible_row_to_entry_index(0), Some(0));
    // Row 1 -> visible_indices[1] = 1 (docs)
    assert_eq!(tab.visible_row_to_entry_index(1), Some(1));
    // Row 2 -> visible_indices[2] = 2 (src)
    assert_eq!(tab.visible_row_to_entry_index(2), Some(2));
    // Row 3 -> visible_indices[3] = 5 (main.rs)
    assert_eq!(tab.visible_row_to_entry_index(3), Some(5));
    // Row 4 -> visible_indices[4] = 6 (lib.rs)
    assert_eq!(tab.visible_row_to_entry_index(4), Some(6));
    // Out of bounds
    assert_eq!(tab.visible_row_to_entry_index(5), None);
}

#[tokio::test]
async fn test_visible_row_to_entry_index_without_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().scroll_offset = 0;

    let tab = app.active_tab();
    // Without filter, row maps directly to entry index
    assert_eq!(tab.visible_row_to_entry_index(0), Some(0));
    assert_eq!(tab.visible_row_to_entry_index(3), Some(3));
    assert_eq!(tab.visible_row_to_entry_index(7), None); // out of bounds
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_filter_matching_no_file_keeps_directories() {
    let mut app = test_app(mixed_entries());
    // A valid glob that matches no file in this directory
    app.active_tab_mut().set_file_filter(Some("zzz*")).unwrap();

    let tab = app.active_tab();
    // Only the directories remain visible
    assert_eq!(tab.visible_indices, vec![0, 1, 2]);
    assert_eq!(tab.visible_file_count(), 0);
}

#[tokio::test]
async fn test_filter_match_all_entries() {
    let mut app = test_app(mixed_entries());
    // * matches everything, so all files stay visible too
    app.active_tab_mut().set_file_filter(Some("*")).unwrap();

    let tab = app.active_tab();
    assert_eq!(tab.visible_count(), 7); // all entries
}

#[tokio::test]
async fn test_filter_shows_all_dirs_plus_matching_files() {
    let mut app = test_app(mixed_entries());
    // "ma" matches only the main.rs file, no directory at all
    app.active_tab_mut().set_file_filter(Some("ma")).unwrap();

    let tab = app.active_tab();
    // Every directory is visible, plus the one matching file
    assert_eq!(tab.visible_indices, vec![0, 1, 2, 5]); // [.., docs, src, main.rs]
    assert_eq!(tab.visible_file_count(), 1);
}

#[tokio::test]
async fn test_pattern_is_wrapped_in_wildcards() {
    let entries = vec![
        entry("..", true),
        entry("photo.jpg", false),
        entry("jpeg.txt", false),
        entry("my_jpg_folder", true),
    ];
    let mut app = test_app(entries);
    // A bare substring is implicitly wrapped in *...*: jpg -> *jpg*
    app.active_tab_mut().set_file_filter(Some("jpg")).unwrap();

    let tab = app.active_tab();
    assert!(tab.is_visible(1)); // photo.jpg contains "jpg"
    // The match is a substring, not a prefix: "jpeg" has no "jpg" in it
    assert!(!tab.is_visible(2));
    // The directory is visible regardless of the pattern
    assert!(tab.is_visible(3)); // my_jpg_folder
    assert_eq!(tab.visible_count(), 3);
}

#[tokio::test]
async fn test_filter_toggle_on_off() {
    let mut app = test_app(mixed_entries());

    // Apply filter
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    assert_eq!(app.active_tab().visible_count(), 5);

    // Clear filter
    app.active_tab_mut().clear_file_filter();
    assert_eq!(app.active_tab().visible_count(), 7);

    // Apply a different filter
    app.active_tab_mut()
        .set_file_filter(Some("Cargo*"))
        .unwrap();
    assert_eq!(app.active_tab().visible_count(), 4); // [.., docs, src, Cargo.toml]
}

#[tokio::test]
async fn test_cursor_is_a_position_in_the_visible_listing() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    // visible_indices: [0, 1, 2, 5, 6]
    // The cursor is a row in that listing; the entry is resolved through it.
    for (row, entry_index) in [(0usize, 0usize), (1, 1), (3, 5), (4, 6)] {
        app.active_tab_mut().cursor = row;
        let tab = app.active_tab();
        assert_eq!(tab.cursor_entry_index(), Some(entry_index));
        assert_eq!(tab.entry_index_to_visible_row(entry_index), Some(row));
        // The row-to-entry map is the inverse of the entry-to-row map.
        assert_eq!(tab.visible_row_to_entry_index(row), Some(entry_index));
    }
}
