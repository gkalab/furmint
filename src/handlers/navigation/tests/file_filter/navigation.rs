use super::*;
use crate::handlers::navigation::{
    handle_down, handle_down_search, handle_end, handle_home, handle_page_down, handle_page_up,
    handle_type_char, handle_up, handle_up_search,
};

// ---------------------------------------------------------------------------
// Navigation with filter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_up_filtered_skips_hidden() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    // visible_indices: [0, 1, 2, 5, 6]
    // Cursor on lib.rs (entry 6, visible row 4)
    app.active_tab_mut().cursor = 4;
    handle_up(&mut app).await;
    // Should go to main.rs (entry 5, visible row 3)
    assert_eq!(
        app.active_tab().current_entry().map(|e| e.name.as_str()),
        Some("main.rs")
    );
}

#[tokio::test]
async fn test_down_filtered_skips_hidden() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    // visible_indices: [0, 1, 2, 5, 6]
    // Cursor on ".." (visible row 0)
    app.active_tab_mut().cursor = 0;
    handle_down(&mut app).await;
    // Should go to docs (entry 1, visible row 1)
    assert_eq!(
        app.active_tab().current_entry().map(|e| e.name.as_str()),
        Some("docs")
    );
}

#[tokio::test]
async fn test_home_filtered_goes_to_first_visible() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    app.active_tab_mut().cursor = 4;
    handle_home(&mut app).await;
    // First visible is ".."
    assert_eq!(
        app.active_tab().current_entry().map(|e| e.name.as_str()),
        Some("..")
    );
}

#[tokio::test]
async fn test_end_filtered_goes_to_last_visible() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    app.active_tab_mut().cursor = 0;
    handle_end(&mut app).await;
    // Last visible is lib.rs (entry 6, visible row 4)
    assert_eq!(
        app.active_tab().current_entry().map(|e| e.name.as_str()),
        Some("lib.rs")
    );
}

#[tokio::test]
async fn test_page_up_filtered() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    // visible_indices: [0, 1, 2, 5, 6] (5 entries)
    // Cursor on lib.rs (visible row 4)
    app.active_tab_mut().cursor = 4;
    handle_page_up(&mut app).await;
    // Page size is 20, but only 5 visible, so should go to first visible
    assert_eq!(
        app.active_tab().current_entry().map(|e| e.name.as_str()),
        Some("..")
    );
}

#[tokio::test]
async fn test_page_down_filtered() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    // Cursor on ".." (visible row 0)
    app.active_tab_mut().cursor = 0;
    handle_page_down(&mut app).await;
    // Page size 20 > 5 visible, should clamp to last visible
    assert_eq!(
        app.active_tab().current_entry().map(|e| e.name.as_str()),
        Some("lib.rs")
    );
}

// ---------------------------------------------------------------------------
// Search interaction with filter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_search_respects_file_filter() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    // Type "Z" ‒ should NOT match any visible entry (no visible name contains
    // a 'z')
    handle_type_char(&mut app, 'Z').await;
    let tab = app.active_tab();
    // No visible entry fuzzy-matches "z", so matching_indices should be empty
    assert!(
        tab.search.matching_indices.is_empty(),
        "expected no matches for 'Z', got {:?}",
        tab.search.matching_indices
    );
}

#[tokio::test]
async fn test_search_finds_only_visible_files() {
    let mut app = test_app(mixed_entries());
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();

    // Type "m" ‒ should match main.rs (entry 5) via prefix match
    handle_type_char(&mut app, 'm').await;
    let tab = app.active_tab();
    assert_eq!(tab.search.matching_indices, vec![5]);
    assert_eq!(tab.cursor_entry_index(), Some(5));
}

#[tokio::test]
async fn test_search_navigation_with_filter() {
    // Use names so search is unambiguous ("m" appears only in mike.rs)
    let entries = vec![
        entry("..", true),
        entry("one", true),
        entry("two", true),
        entry("one.txt", false),
        entry("two.txt", false),
        entry("mike.rs", false),
        entry("zoe.rs", false),
    ];
    let mut app = test_app(entries);
    // Filter to only .rs files (dirs always stay visible)
    app.active_tab_mut().set_file_filter(Some("*.rs")).unwrap();
    // visible: [0="..", 1=one, 2=two, 5=mike.rs, 6=zoe.rs]

    // Type "m" ‒ matches only mike.rs (entry 5)
    handle_type_char(&mut app, 'm').await;
    assert_eq!(app.active_tab().cursor_entry_index(), Some(5));

    // Down search ‒ only one match, so wraps to itself
    handle_down_search(&mut app).await;
    assert_eq!(app.active_tab().cursor_entry_index(), Some(5));

    // Up search ‒ same
    handle_up_search(&mut app).await;
    assert_eq!(app.active_tab().cursor_entry_index(), Some(5));
}
