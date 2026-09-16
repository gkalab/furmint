use fm::state::FileViewerState;
use std::str::FromStr;

#[test]
fn test_display_col_to_char_idx() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["\tA\tB".to_string(), "Wide: 🚀".to_string()];

    // Tab at 0 expands to 4 spaces (0, 1, 2, 3). Display pos 4 is 'A'.
    assert_eq!(state.display_col_to_char_idx(0, 0), 0);
    assert_eq!(state.display_col_to_char_idx(0, 3), 0);
    assert_eq!(state.display_col_to_char_idx(0, 4), 1); // 'A'
    assert_eq!(state.display_col_to_char_idx(0, 5), 2); // Tab at index 2
    // Next tab starts at display pos 5. 5 % 4 = 1. Width = 4 - 1 = 3.
    // Positions 5, 6, 7 are the second tab.
    assert_eq!(state.display_col_to_char_idx(0, 7), 2);
    assert_eq!(state.display_col_to_char_idx(0, 8), 3); // 'B'

    // Wide character
    // "Wide: " is 6 chars. 🚀 is width 2.
    assert_eq!(state.display_col_to_char_idx(1, 5), 5); // ':'
    assert_eq!(state.display_col_to_char_idx(1, 6), 6); // '🚀' (first column)
    assert_eq!(state.display_col_to_char_idx(1, 7), 6); // '🚀' (second column)
    assert_eq!(state.display_col_to_char_idx(1, 8), 7); // End of line
}

#[test]
fn test_select_word_at_fallback() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["hello world_123 !!!".to_string()];
    state.text.language = lumis::languages::Language::default(); // No syntax

    // Select 'hello'
    state.select_word_at(0, 0);
    assert_eq!(state.text.selection, Some(((0, 0), (0, 5))));

    // Select 'world_123'
    state.select_word_at(0, 6);
    assert_eq!(state.text.selection, Some(((0, 6), (0, 15))));

    // Select '!!!' (non-word chunk)
    state.select_word_at(0, 16);
    assert_eq!(state.text.selection, Some(((0, 15), (0, 19))));
}

#[test]
fn test_select_word_at_quotes() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["\"\"\"triple\"\"\" '\"nested\"' `backtick`".to_string()];
    state.text.language = lumis::languages::Language::default();

    // Select triple
    // """triple""" is at 0-12. triple is at 3-9.
    state.select_word_at(0, 5);
    assert_eq!(state.text.selection, Some(((0, 3), (0, 9))));

    // Select nested
    // '"nested"' is at 13-22 (with space at 12).
    // ' (13), " (14), n (15) ... d (20), " (21), ' (22)
    state.select_word_at(0, 17);
    assert_eq!(state.text.selection, Some(((0, 15), (0, 21))));

    // Select backtick
    state.select_word_at(0, 28);
    assert_eq!(state.text.selection, Some(((0, 25), (0, 33))));

    // Single quote/double quote not matching -> select word only
    state.text.content = vec!["\"no match'".to_string()];
    state.select_word_at(0, 1);
    assert_eq!(state.text.selection, Some(((0, 1), (0, 3))));
}

#[test]
fn test_select_word_at_syntax_quotes() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.path = std::path::PathBuf::from("test.json");
    // Using from_str as it's safer for different lumis versions
    state.text.language = lumis::languages::Language::from_str("json").unwrap_or_default();
    state.text.content = vec!["{ \"key\": \"value\" }".to_string()];

    // index 10 is 'v' in "value"
    state.select_word_at(0, 10);
    // "value" is at indices 9-16. Refinement should strip quotes to 10-15.
    // NOTE: This test depends on lumis grouping "value" as a single segment.
    // If it doesn't, fallback will take over, which should also work.
    if let Some(((r, s), (re, e))) = state.text.selection {
        assert_eq!(r, 0);
        assert_eq!(re, 0);
        assert_eq!(s, 10);
        assert_eq!(e, 15);
    } else {
        panic!("Selection should not be None");
    }
}

#[test]
fn test_search_basic() {
    let mut state = FileViewerState::new(true, "test");
    state.area = ratatui::layout::Rect::new(0, 0, 80, 20); // 20 lines viewport
    state.text.content = vec![
        "hello world".to_string(),
        "this is me".to_string(),
        "another line".to_string(),
        "me too".to_string(),
    ];

    state.search("me");
    // "this is me" is at line 1. Visible, so no scroll.
    assert_eq!(state.text.current_search_match, Some((1, 8, 10)));
    assert_eq!(state.text.scroll_offset, 0);

    state.search_next();
    // "me too" is at line 3. Visible, so no scroll.
    assert_eq!(state.text.current_search_match, Some((3, 0, 2)));
    assert_eq!(state.text.scroll_offset, 0);

    // Move viewport away
    state.text.scroll_offset = 10;
    state.search_next();
    // Wrap around to line 1. NOT visible (1 < 10), so jump with context.
    assert_eq!(state.text.current_search_match, Some((1, 8, 10)));
    assert_eq!(state.text.scroll_offset, 0); // 1 - 4
}

#[test]
fn test_search_regex() {
    let mut state = FileViewerState::new(true, "test");
    state.area = ratatui::layout::Rect::new(0, 0, 80, 20);
    state.text.content = vec!["abc123def".to_string(), "ghi456jkl".to_string()];

    state.search("\\d+");
    assert_eq!(state.text.current_search_match, Some((0, 3, 6)));

    state.search_next();
    assert_eq!(state.text.current_search_match, Some((1, 3, 6)));
}

#[test]
fn test_search_case_insensitive() {
    let mut state = FileViewerState::new(true, "test");
    state.area = ratatui::layout::Rect::new(0, 0, 80, 20);
    state.text.content = vec!["Hello".to_string(), "world".to_string()];

    state.search("hello");
    assert_eq!(state.text.current_search_match, Some((0, 0, 5)));
}

#[test]
fn test_search_empty() {
    let mut state = FileViewerState::new(true, "test");
    state.text.content = vec!["abc".to_string()];
    state.search("");
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
        fm::large_text::file_reader::FileReader::new(test_file, encoding_rs::UTF_8).unwrap();
    let mut indexer = fm::large_text::line_indexer::LineIndexer::new();
    indexer.index_file(&reader);

    let mut state = FileViewerState::new(true, "test");
    state.area = ratatui::layout::Rect::new(0, 0, 80, 20);
    state.text.content = Vec::new();
    state.text.large_file_reader = Some(reader);
    state.text.large_file_indexer = Some(indexer);

    state.search("marker");
    assert_eq!(state.text.current_search_match, Some((500, 6, 12)));

    state.search_next();
    assert_eq!(state.text.current_search_match, Some((50_000, 7, 13)));

    state.search_next();
    assert_eq!(state.text.current_search_match, Some((99_999, 5, 11)));

    state.search_next(); // wraps to top
    assert_eq!(state.text.current_search_match, Some((500, 6, 12)));

    state.search_prev(); // wraps to bottom
    assert_eq!(state.text.current_search_match, Some((99_999, 5, 11)));

    state.search_prev();
    assert_eq!(state.text.current_search_match, Some((50_000, 7, 13)));

    // No match: full-file scan.
    assert!(!state.search("nomatch"));
    assert_eq!(state.text.current_search_match, None);
}

#[test]
fn test_search_prev() {
    let mut state = FileViewerState::new(true, "test");
    state.area = ratatui::layout::Rect::new(0, 0, 80, 20);
    state.text.content = vec![
        "match 1".to_string(),
        "match 2".to_string(),
        "match 3".to_string(),
    ];
    state.text.scroll_offset = 2;
    state.search("match");
    // Starts from scroll_offset 2, so finds "match 3" at line 2.
    assert_eq!(state.text.current_search_match, Some((2, 0, 5)));

    state.search_prev();
    assert_eq!(state.text.current_search_match, Some((1, 0, 5)));

    state.search_prev();
    assert_eq!(state.text.current_search_match, Some((0, 0, 5)));

    state.search_prev(); // Wrap to bottom
    assert_eq!(state.text.current_search_match, Some((2, 0, 5)));
}
