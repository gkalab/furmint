use fm::app::AppState;
use fm::app_state::tabs::{PanelSide, TabManager};
use fm::fs::utils::FileEntry;
use fm::handlers::navigation::{
    handle_directory_up, handle_down, handle_down_search, handle_end, handle_enter_directory,
    handle_home, handle_open_item, handle_page_down, handle_page_up, handle_sort, handle_tab,
    handle_toggle_selection, handle_type_char, handle_up, handle_up_search, reset_expired_search,
    reset_search, update_viewer_content,
};

fn test_app(entries: Vec<FileEntry>) -> AppState {
    let mut tab = fm::test_utils::create_test_tab();
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
    fm::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(right_tm)
        .build()
}

#[tokio::test]
async fn test_handle_up_moves_cursor() {
    let entry = FileEntry {
        name: "one".to_string(),
        is_dir: false,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    };
    let mut app = test_app(vec![entry.clone(); 3]);
    app.panels.left.active_tab_mut().cursor = 2;
    handle_up(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 1);
}

#[tokio::test]
async fn test_handle_down_moves_cursor() {
    let entry = FileEntry {
        name: "one".to_string(),
        is_dir: false,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    };
    let mut app = test_app(vec![entry.clone(); 3]);
    handle_down(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 1);
}

#[tokio::test]
async fn test_handle_page_up_down() {
    let entries = vec![
        FileEntry {
            name: "file".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false
        };
        50
    ];
    let mut app = test_app(entries);
    app.panels.left.active_tab_mut().cursor = 45;
    handle_page_up(&mut app).await;
    assert!(app.panels.left.active_tab().cursor < 45);
    handle_page_down(&mut app).await;
    assert!(app.panels.left.active_tab().cursor > 0);
}

#[tokio::test]
async fn test_handle_home_end() {
    let entries = vec![
        FileEntry {
            name: "file".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false
        };
        10
    ];
    let mut app = test_app(entries);
    app.panels.left.active_tab_mut().cursor = 4;
    handle_home(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 0);
    handle_end(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 9);
}

#[tokio::test]
async fn test_handle_enter_directory_only_enters_dirs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path();
    let dir_path = path.join("test_dir");
    std::fs::create_dir(&dir_path).unwrap();
    let file_path = path.join("test_file");
    std::fs::write(&file_path, "test").unwrap();

    let mut app = test_app(vec![]);
    app.panels.left.active_tab_mut().current_dir = path.to_path_buf();
    // Manually list to populate entries
    app.panels
        .left
        .active_tab_mut()
        .navigate_to(path)
        .await
        .unwrap();

    // Find file and dir indices
    let file_idx = app
        .panels
        .left
        .active_tab()
        .entries
        .iter()
        .position(|e| e.name == "test_file")
        .unwrap();
    let dir_idx = app
        .panels
        .left
        .active_tab()
        .entries
        .iter()
        .position(|e| e.name == "test_dir")
        .unwrap();

    // Try to enter a file
    app.panels.left.active_tab_mut().cursor = file_idx;
    let original_dir = app.panels.left.active_tab().current_dir.clone();
    handle_enter_directory(&mut app).await;
    assert_eq!(
        app.panels.left.active_tab().current_dir,
        original_dir,
        "Should NOT enter a file"
    );

    // Try to enter a directory
    app.panels.left.active_tab_mut().cursor = dir_idx;
    handle_enter_directory(&mut app).await;
    assert_ne!(
        app.panels.left.active_tab().current_dir,
        original_dir,
        "Should enter a directory"
    );
    assert!(
        app.panels
            .left
            .active_tab()
            .current_dir
            .ends_with("test_dir")
    );

    // Test handle_open_item on file (should stay in same dir as it spawns process)
    #[cfg(not(windows))]
    {
        app.panels
            .left
            .active_tab_mut()
            .navigate_to(path)
            .await
            .unwrap();
        app.panels.left.active_tab_mut().cursor = file_idx;
        let original_dir = app.panels.left.active_tab().current_dir.clone();
        handle_open_item(&mut app).await;
        assert_eq!(
            app.panels.left.active_tab().current_dir,
            original_dir,
            "handle_open_item on file should not change directory"
        );
    }

    // Test handle_open_item on directory (should enter)
    app.panels.left.active_tab_mut().cursor = dir_idx;
    handle_open_item(&mut app).await;
    assert_ne!(
        app.panels.left.active_tab().current_dir,
        original_dir,
        "handle_open_item on directory should change directory"
    );
    assert!(
        app.panels
            .left
            .active_tab()
            .current_dir
            .ends_with("test_dir")
    );
}

#[tokio::test]
async fn test_handle_type_char() {
    let entries = vec![
        FileEntry {
            name: "apple".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "banana".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    handle_type_char(&mut app, 'b').await;
    assert_eq!(app.panels.left.active_tab().cursor, 1);

    // Test buffer reset (we can't easily wait 1s in unit test, but we can check it appends)
    handle_type_char(&mut app, 'a').await;
    // 'ba' doesn't match anything, so cursor should stay at 1 (previous match)
    assert_eq!(app.panels.left.active_tab().cursor, 1);

    app.panels.left.active_tab_mut().search.buffer.clear();
    handle_type_char(&mut app, 'a').await;
    assert_eq!(app.panels.left.active_tab().cursor, 0);
}

#[tokio::test]
async fn test_handle_type_char_fuzzy_fallback() {
    let entries = vec![
        FileEntry {
            name: "apple.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "banana.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "Cargo.toml".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    // Type 'c', 't' -> Should match 'Cargo.toml' (fuzzy)
    handle_type_char(&mut app, 'c').await;
    assert_eq!(app.active_tab().cursor, 2); // 'Cargo.toml' starts with 'c'

    handle_type_char(&mut app, 't').await;
    // 'ct' matches 'Cargo.toml' fuzzy, but doesn't start with 'ct'
    assert_eq!(app.active_tab().cursor, 2);
    assert!(
        !app.active_tab().entries[2]
            .name
            .to_lowercase()
            .starts_with("ct")
    );
    assert!(app.active_tab().search.matching_indices.contains(&2));

    // Test with something that ONLY matches fuzzy
    app.panels.left.active_tab_mut().search.buffer.clear();
    handle_type_char(&mut app, 'b').await;
    handle_type_char(&mut app, 't').await; // 'bt' doesn't match 'apple', 'banana', 'Cargo' via prefix
    // 'bt' matches 'banana.txt' (b...t) and 'apple.txt' (p...t...x...t)
    // 'banana.txt' should rank higher for 'bt'
    assert_eq!(app.active_tab().cursor, 1); // 'banana.txt'
}

#[tokio::test]
async fn test_handle_tab() {
    let mut app = test_app(vec![]);
    assert_eq!(app.panels.active, PanelSide::Left);

    handle_tab(&mut app);
    assert_eq!(app.panels.active, PanelSide::Right);

    handle_tab(&mut app);
    assert_eq!(app.panels.active, PanelSide::Left);

    // Test focusing file viewer
    app.file_viewer.is_visible = true;
    app.file_viewer.focused = false;
    handle_tab(&mut app);
    assert!(app.file_viewer.focused);
}

#[tokio::test]
async fn test_handle_sort_and_toggle() {
    let entries = vec![FileEntry {
        name: "a".to_string(),
        is_dir: false,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    }];
    let mut app = test_app(entries);

    handle_toggle_selection(&mut app).await;
    assert!(app.panels.left.active_tab().entries[0].selected);

    handle_sort(&mut app, fm::app_state::tabs::SortColumn::Size).await;
    assert_eq!(
        app.panels.left.active_tab().sort.column,
        fm::app_state::tabs::SortColumn::Size
    );
}

#[tokio::test]
async fn test_handle_directory_up() {
    // This test is limited because go_up requires actual filesystem or complex mocking
    // But we can at least call it to see it doesn't panic and cover the handler lines.
    let mut app = test_app(vec![]);
    handle_directory_up(&mut app).await;
}

#[tokio::test]
async fn test_handle_type_char_populates_matching_indices() {
    let entries = vec![
        FileEntry {
            name: "ab".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abcd".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abce".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "ace".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    handle_type_char(&mut app, 'a').await;
    handle_type_char(&mut app, 'b').await;

    // Should have 3 matches: ab, abcd, abce
    assert_eq!(
        app.panels.left.active_tab().search.matching_indices.len(),
        3
    );
    assert_eq!(app.panels.left.active_tab().search.position, 0);
    // First match (ab) should be selected
    assert_eq!(app.panels.left.active_tab().cursor, 0);
}

#[tokio::test]
async fn test_search_navigation_multiple_matches() {
    let entries = vec![
        FileEntry {
            name: "ab".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abcd".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abce".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "ace".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    // Type 'a', 'b' -> cursor on 'ab' (index 0)
    handle_type_char(&mut app, 'a').await;
    handle_type_char(&mut app, 'b').await;
    assert_eq!(app.panels.left.active_tab().cursor, 0);
    assert_eq!(app.panels.left.active_tab().search.position, 0);

    // Down -> cursor on 'abcd' (index 1)
    handle_down_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 1);
    assert_eq!(app.panels.left.active_tab().search.position, 1);

    // Down -> cursor on 'abce' (index 2)
    handle_down_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 2);
    assert_eq!(app.panels.left.active_tab().search.position, 2);

    // Up -> cursor back on 'abcd' (index 1)
    handle_up_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 1);
    assert_eq!(app.panels.left.active_tab().search.position, 1);

    // Up -> cursor on 'ab' (index 0)
    handle_up_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 0);
    assert_eq!(app.panels.left.active_tab().search.position, 0);

    // Up -> wraps to 'abce' (index 2)
    handle_up_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 2);
    assert_eq!(app.panels.left.active_tab().search.position, 2);
}

#[tokio::test]
async fn test_search_navigation_wrap_around() {
    let entries = vec![
        FileEntry {
            name: "aaa".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "aab".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "aac".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    handle_type_char(&mut app, 'a').await;
    handle_type_char(&mut app, 'a').await;

    // At first match (aaa)
    assert_eq!(app.panels.left.active_tab().cursor, 0);
    assert_eq!(app.panels.left.active_tab().search.position, 0);

    // Down twice to get to last match (aac)
    handle_down_search(&mut app).await;
    handle_down_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 2);
    assert_eq!(app.panels.left.active_tab().search.position, 2);

    // Down again wraps to first (aaa)
    handle_down_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 0);
    assert_eq!(app.panels.left.active_tab().search.position, 0);
}

#[tokio::test]
async fn test_search_single_match_ignores_arrows() {
    let entries = vec![
        FileEntry {
            name: "xyz".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abc".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    handle_type_char(&mut app, 'x').await;

    assert_eq!(app.panels.left.active_tab().cursor, 0);
    assert_eq!(
        app.panels.left.active_tab().search.matching_indices.len(),
        1
    );
    assert_eq!(app.panels.left.active_tab().search.position, 0);

    // Down -> stays on 'xyz'
    handle_down_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 0);

    // Up -> stays on 'xyz'
    handle_up_search(&mut app).await;
    assert_eq!(app.panels.left.active_tab().cursor, 0);
}

#[tokio::test]
async fn test_esc_resets_search() {
    let entries = vec![
        FileEntry {
            name: "apple".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "banana".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    handle_type_char(&mut app, 'b').await;
    assert_eq!(app.panels.left.active_tab().cursor, 1);
    assert!(!app.panels.left.active_tab().search.buffer.is_empty());

    reset_search(&mut app);

    assert!(app.panels.left.active_tab().search.buffer.is_empty());
    assert!(
        app.panels
            .left
            .active_tab()
            .search
            .matching_indices
            .is_empty()
    );
    assert_eq!(app.panels.left.active_tab().search.position, 0);
    assert!(app.panels.left.active_tab().search.last_type_time.is_none());
}

#[tokio::test]
async fn test_search_restarts_timer() {
    let entries = vec![
        FileEntry {
            name: "ab".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abcd".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abce".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    handle_type_char(&mut app, 'a').await;
    handle_type_char(&mut app, 'b').await;

    let first_type_time = app.panels.left.active_tab().search.last_type_time;

    // Navigate down (should restart timer)
    handle_down_search(&mut app).await;

    let second_type_time = app.panels.left.active_tab().search.last_type_time;
    assert!(second_type_time > first_type_time);
}

#[tokio::test]
async fn test_timeout_resets_search_state() {
    let entries = vec![
        FileEntry {
            name: "ab".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abcd".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "xyz".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    handle_type_char(&mut app, 'a').await;
    handle_type_char(&mut app, 'b').await;

    // Verify search is active
    assert!(!app.panels.left.active_tab().search.buffer.is_empty());
    assert_eq!(
        app.panels.left.active_tab().search.matching_indices.len(),
        2
    );
    assert_eq!(app.panels.left.active_tab().cursor, 0);

    // Simulate timeout by setting last_type_time to old value
    app.panels.left.active_tab_mut().search.last_type_time = Some(
        std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(2))
            .unwrap(),
    );

    // Call reset_search as event loop would when timeout expired
    reset_search(&mut app);

    // Verify search state is cleared
    assert!(app.panels.left.active_tab().search.buffer.is_empty());
    assert!(
        app.panels
            .left
            .active_tab()
            .search
            .matching_indices
            .is_empty()
    );
    assert_eq!(app.panels.left.active_tab().search.position, 0);
    assert!(app.panels.left.active_tab().search.last_type_time.is_none());
}

#[tokio::test]
async fn test_periodic_reset_expired_search() {
    let entries = vec![
        FileEntry {
            name: "ab".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "abcd".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    handle_type_char(&mut app, 'a').await;
    handle_type_char(&mut app, 'b').await;

    // Verify search is active
    assert!(!app.panels.left.active_tab().search.buffer.is_empty());
    assert_eq!(
        app.panels.left.active_tab().search.matching_indices.len(),
        2
    );

    // Simulate timeout by setting last_type_time to old value
    app.panels.left.active_tab_mut().search.last_type_time = Some(
        std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(2))
            .unwrap(),
    );

    // Call reset_expired_search as periodic check would
    reset_expired_search(&mut app);

    // Verify search state is cleared
    assert!(app.panels.left.active_tab().search.buffer.is_empty());
    assert!(
        app.panels
            .left
            .active_tab()
            .search
            .matching_indices
            .is_empty()
    );
    assert_eq!(app.panels.left.active_tab().search.position, 0);
}

#[tokio::test]
async fn test_handle_type_char_populates_highlights() {
    let entries = vec![
        FileEntry {
            name: "test_file.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "other.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    ];
    let mut app = test_app(entries);

    // 1. Prefix match "test"
    for c in "test".chars() {
        handle_type_char(&mut app, c).await;
    }

    let panel = app.active_tab();
    assert!(!panel.search.matching_indices.is_empty());
    assert!(panel.search.highlights.contains_key(&0)); // "test_file.txt" is at index 0
    let highlights = panel.search.highlights.get(&0).unwrap();
    assert_eq!(highlights, &vec![0, 1, 2, 3]);

    // Reset
    reset_search(&mut app);
    assert!(app.active_tab().search.highlights.is_empty());

    // 2. Fuzzy match "tf"
    handle_type_char(&mut app, 't').await;
    handle_type_char(&mut app, 'f').await;

    let panel = app.active_tab();
    assert!(!panel.search.matching_indices.is_empty());
    assert!(panel.search.highlights.contains_key(&0));
    let highlights = panel.search.highlights.get(&0).unwrap();
    // Fuzzy match should highlight 't' (0) and 'f' (5)
    assert!(highlights.contains(&0));
    assert!(highlights.contains(&5));
}

#[tokio::test]
async fn test_update_viewer_content_loads_large_file() {
    use std::io::Write;
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("large.txt");
    let mut f = std::fs::File::create(&file_path).unwrap();
    let mb = 11;
    // Create a file with distinct lines to verify indexing
    for i in 0..1000 {
        writeln!(f, "Line {i:04}").unwrap();
    }
    // Fill the rest to reach 11MB
    f.write_all(&vec![b'a'; mb * 1024 * 1024]).unwrap();

    let mut app = test_app(vec![FileEntry {
        name: "large.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(std::fs::metadata(&file_path).unwrap().len()),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.file_viewer.is_visible = true;
    update_viewer_content(&mut app).await;

    // Should NOT have error message in content
    assert!(app.file_viewer.content.is_empty());
    // Should have large file components initialized
    assert!(app.file_viewer.large_file_reader.is_some());
    assert!(app.file_viewer.large_file_indexer.is_some());

    // Verify we can read the first line
    let indexer = app.file_viewer.large_file_indexer.as_ref().unwrap();
    let reader = app.file_viewer.large_file_reader.as_ref().unwrap();
    let (s, e) = indexer.get_line_with_reader(0, reader).unwrap();
    assert_eq!(reader.get_chunk(s, e).trim(), "Line 0000");
}

#[tokio::test]
async fn test_update_viewer_content_shows_error_for_binary() {
    use std::io::Write;
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("bin.dat");
    let mut f = std::fs::File::create(&file_path).unwrap();
    let mut data = vec![0u8; 9000];
    data[10] = 0; // null byte early in file
    f.write_all(&data).unwrap();
    let mut app = test_app(vec![FileEntry {
        name: "bin.dat".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(9000),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.file_viewer.is_visible = true;
    update_viewer_content(&mut app).await;
    let msg = &app.file_viewer.content[0];

    assert!(
        msg.to_lowercase().contains("binary"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn test_update_viewer_content_reads_text_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("hello.txt");
    std::fs::write(&file_path, "Hello, F3!").unwrap();
    let mut app = test_app(vec![FileEntry {
        name: "hello.txt".to_string(),

        is_dir: false,
        is_symlink: false,
        size: Some(11),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.file_viewer.is_visible = true;
    update_viewer_content(&mut app).await;
    assert_eq!(app.file_viewer.content[0], "Hello, F3!");
}
