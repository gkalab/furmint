use fm::state::FileViewerState;
use std::str::FromStr;

#[test]
fn test_display_col_to_char_idx() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.content = vec!["\tA\tB".to_string(), "Wide: 🚀".to_string()];

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
    state.content = vec!["hello world_123 !!!".to_string()];
    state.language = lumis::languages::Language::default(); // No syntax

    // Select 'hello'
    state.select_word_at(0, 0);
    assert_eq!(state.selection, Some(((0, 0), (0, 5))));

    // Select 'world_123'
    state.select_word_at(0, 6);
    assert_eq!(state.selection, Some(((0, 6), (0, 15))));

    // Select '!!!' (non-word chunk)
    state.select_word_at(0, 16);
    assert_eq!(state.selection, Some(((0, 15), (0, 19))));
}

#[test]
fn test_select_word_at_quotes() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.content = vec!["\"\"\"triple\"\"\" '\"nested\"' `backtick`".to_string()];
    state.language = lumis::languages::Language::default();

    // Select triple
    // """triple""" is at 0-12. triple is at 3-9.
    state.select_word_at(0, 5);
    assert_eq!(state.selection, Some(((0, 3), (0, 9))));

    // Select nested
    // '"nested"' is at 13-22 (with space at 12).
    // ' (13), " (14), n (15) ... d (20), " (21), ' (22)
    state.select_word_at(0, 17);
    assert_eq!(state.selection, Some(((0, 15), (0, 21))));

    // Select backtick
    state.select_word_at(0, 28);
    assert_eq!(state.selection, Some(((0, 25), (0, 33))));

    // Single quote/double quote not matching -> select word only
    state.content = vec!["\"no match'".to_string()];
    state.select_word_at(0, 1);
    assert_eq!(state.selection, Some(((0, 1), (0, 3))));
}

#[test]
fn test_select_word_at_syntax_quotes() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.path = std::path::PathBuf::from("test.json");
    // Using from_str as it's safer for different lumis versions
    state.language = lumis::languages::Language::from_str("json").unwrap_or_default();
    state.content = vec!["{ \"key\": \"value\" }".to_string()];

    // index 10 is 'v' in "value"
    state.select_word_at(0, 10);
    // "value" is at indices 9-16. Refinement should strip quotes to 10-15.
    // NOTE: This test depends on lumis grouping "value" as a single segment.
    // If it doesn't, fallback will take over, which should also work.
    if let Some(((r, s), (re, e))) = state.selection {
        assert_eq!(r, 0);
        assert_eq!(re, 0);
        assert_eq!(s, 10);
        assert_eq!(e, 15);
    } else {
        panic!("Selection should not be None");
    }
}
