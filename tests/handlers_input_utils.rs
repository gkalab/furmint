use fm::handlers::input_utils;
use termina::event::{KeyCode, Modifiers};

#[test]
fn test_keyevent_to_string() {
    assert_eq!(
        input_utils::keyevent_to_string(KeyCode::Function(3), Modifiers::CONTROL),
        "Ctrl-F3"
    );
    assert_eq!(
        input_utils::keyevent_to_string(KeyCode::Char('p'), Modifiers::CONTROL),
        "Ctrl-p"
    );
    assert_eq!(
        input_utils::keyevent_to_string(KeyCode::Left, Modifiers::ALT),
        "Alt-Left"
    );
}
