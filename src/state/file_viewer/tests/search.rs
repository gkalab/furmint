use super::*;

#[test]
fn test_search_basic() {
    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = vec![
        "hello world".to_string(),
        "this is me".to_string(),
        "another line".to_string(),
        "me too".to_string(),
    ];

    state.search("me", &geometry);
    // "this is me" is at line 1. Visible, so no scroll.
    assert_eq!(state.text.current_search_match, Some((1, 8, 10)));
    assert_eq!(state.text.scroll_offset, 0);

    state.search_next(&geometry);
    // "me too" is at line 3. Visible, so no scroll.
    assert_eq!(state.text.current_search_match, Some((3, 0, 2)));
    assert_eq!(state.text.scroll_offset, 0);

    // Move viewport away
    state.text.scroll_offset = 10;
    state.search_next(&geometry);
    // Wrap around to line 1. NOT visible (1 < 10), so jump with context.
    assert_eq!(state.text.current_search_match, Some((1, 8, 10)));
    assert_eq!(state.text.scroll_offset, 0); // 1 - 4
}

#[test]
fn test_search_regex() {
    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = vec!["abc123def".to_string(), "ghi456jkl".to_string()];

    state.search("\\d+", &geometry);
    assert_eq!(state.text.current_search_match, Some((0, 3, 6)));

    state.search_next(&geometry);
    assert_eq!(state.text.current_search_match, Some((1, 3, 6)));
}

#[test]
fn test_search_case_insensitive() {
    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = vec!["Hello".to_string(), "world".to_string()];

    state.search("hello", &geometry);
    assert_eq!(state.text.current_search_match, Some((0, 0, 5)));
}

#[test]
fn test_search_empty() {
    let mut state = FileViewerState::new(true, "test");
    state.text.content = vec!["abc".to_string()];
    let geometry = viewer_geometry(80, 20);
    state.search("", &geometry);
    assert_eq!(state.text.current_search_match, None);
}

#[test]
fn test_search_large_file_chunked() {
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    let test_file = temp_file.path().to_path_buf();

    // ~3.3MB / 100k lines so the search crosses 2MB decode chunks.
    let mut content = String::new();
    for i in 0..100_000 {
        match i {
            500 => content.push_str("first marker here"),
            50_000 => content.push_str("middle marker here"),
            99_999 => content.push_str("last marker here"),
            _ => content.push_str("filler line for testing purposes"),
        }
        content.push('\n');
    }
    std::fs::write(&test_file, &content).unwrap();

    let reader =
        crate::large_text::file_reader::FileReader::new(&test_file, encoding_rs::UTF_8).unwrap();
    let mut indexer = crate::large_text::line_indexer::LineIndexer::new();
    indexer.index_file(&reader);

    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = Vec::new();
    state.text.large_file_reader = Some(reader);
    state.text.large_file_indexer = Some(indexer);

    state.search("marker", &geometry);
    assert_eq!(state.text.current_search_match, Some((500, 6, 12)));

    state.search_next(&geometry);
    assert_eq!(state.text.current_search_match, Some((50_000, 7, 13)));

    state.search_next(&geometry);
    assert_eq!(state.text.current_search_match, Some((99_999, 5, 11)));

    state.search_next(&geometry); // wraps to top
    assert_eq!(state.text.current_search_match, Some((500, 6, 12)));

    state.search_prev(&geometry); // wraps to bottom
    assert_eq!(state.text.current_search_match, Some((99_999, 5, 11)));

    state.search_prev(&geometry);
    assert_eq!(state.text.current_search_match, Some((50_000, 7, 13)));

    // No match: full-file scan.
    assert!(!state.search("nomatch", &geometry));
    assert_eq!(state.text.current_search_match, None);
}

#[test]
fn test_search_prev() {
    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = vec![
        "match 1".to_string(),
        "match 2".to_string(),
        "match 3".to_string(),
    ];
    state.text.scroll_offset = 2;
    state.search("match", &geometry);
    // Starts from scroll_offset 2, so finds "match 3" at line 2.
    assert_eq!(state.text.current_search_match, Some((2, 0, 5)));

    state.search_prev(&geometry);
    assert_eq!(state.text.current_search_match, Some((1, 0, 5)));

    state.search_prev(&geometry);
    assert_eq!(state.text.current_search_match, Some((0, 0, 5)));

    state.search_prev(&geometry); // Wrap to bottom
    assert_eq!(state.text.current_search_match, Some((2, 0, 5)));
}
