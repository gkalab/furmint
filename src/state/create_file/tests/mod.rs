use super::*;
use termina::event::{KeyCode, Modifiers};

#[test]
fn test_insert_in_the_middle_of_multibyte_text() {
    let mut s = CreateFileState::new();
    s.input_value = TextEditState::with_text("héllo.txt");
    // Caret after "héllo" (5 chars), before the extension. A byte-based cursor
    // would land past the é and corrupt the text.
    s.input_value.set_cursor(5);
    assert!(
        s.input_value
            .handle_key(KeyCode::Char('X'), Modifiers::NONE, false)
    );
    assert_eq!(s.input_value.text(), "hélloX.txt");
    assert_eq!(s.input_value.cursor(), 6);
}
