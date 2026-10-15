use super::*;

/// Rendered the same way as `draw_input_box` with `input_width` = `usize::MAX`.
fn rendered_input(state: &FilterableListState) -> String {
    state
        .input
        .chars()
        .skip(state.cursor_position - 1)
        .take(1000)
        .collect()
}

#[test]
fn cursor_end_uses_char_count_not_bytes() {
    let mut state = FilterableListState::new();
    state.input = "héllo".to_string(); // 5 chars, 6 bytes

    state.move_cursor_end();
    // Old code set the cursor to 6 (byte length), which made
    // chars().skip(6) render the field empty.
    assert_eq!(state.cursor_position, 5);
    assert_eq!(rendered_input(&state), "o");
}

#[test]
fn cursor_right_stops_at_char_count() {
    let mut state = FilterableListState::new();
    state.input = "héllo".to_string();

    for _ in 0..8 {
        state.move_cursor_right();
    }
    // Old code allowed the cursor to reach 6 (byte length)
    assert_eq!(state.cursor_position, 5);

    // Left stays symmetric
    state.move_cursor_left();
    assert_eq!(state.cursor_position, 4);
}

#[test]
fn cursor_movement_pure_ascii_unchanged() {
    let mut state = FilterableListState::new();
    state.input = "hello".to_string();

    state.move_cursor_end();
    assert_eq!(state.cursor_position, 5);
    state.move_cursor_home();
    assert_eq!(state.cursor_position, 0);
    state.move_cursor_right();
    state.move_cursor_right();
    assert_eq!(state.cursor_position, 2);
}
