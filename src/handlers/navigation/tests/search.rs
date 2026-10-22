use super::*;

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
    assert_ne!(app.panels.left.active_tab().search.buffer, "");

    reset_search(&mut app);

    assert_eq!(app.panels.left.active_tab().search.buffer, "");
    assert_eq!(
        app.panels.left.active_tab().search.matching_indices,
        [] as [usize; 0]
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
    assert_ne!(app.panels.left.active_tab().search.buffer, "");
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
    assert_eq!(app.panels.left.active_tab().search.buffer, "");
    assert_eq!(
        app.panels.left.active_tab().search.matching_indices,
        [] as [usize; 0]
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
    assert_ne!(app.panels.left.active_tab().search.buffer, "");
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
    assert_eq!(app.panels.left.active_tab().search.buffer, "");
    assert_eq!(
        app.panels.left.active_tab().search.matching_indices,
        [] as [usize; 0]
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
    assert_ne!(panel.search.matching_indices, [] as [usize; 0]);
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
    assert_ne!(panel.search.matching_indices, [] as [usize; 0]);
    assert!(panel.search.highlights.contains_key(&0));
    let highlights = panel.search.highlights.get(&0).unwrap();
    // Fuzzy match should highlight 't' (0) and 'f' (5)
    assert!(highlights.contains(&0));
    assert!(highlights.contains(&5));
}
