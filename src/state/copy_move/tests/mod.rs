use super::*;
use crate::ui::text_edit::TextEditState;

#[test]
fn test_copymoveaction_equality() {
    assert_eq!(CopyMoveAction::Copy, CopyMoveAction::Copy);
    assert_eq!(CopyMoveAction::Move, CopyMoveAction::Move);
    assert_ne!(CopyMoveAction::Copy, CopyMoveAction::Move);
}

#[test]
fn test_new_and_default_state() {
    let s1 = CopyMoveState::new();
    let s2 = CopyMoveState::default();
    assert!(!s1.is_visible);
    assert_eq!(s1.action, CopyMoveAction::Copy);
    assert_eq!(s1.source_paths.len(), 0);
    assert_eq!(s1.destination_input.text(), "");
    assert_eq!(s1.destination_input.cursor(), 0);
    assert_eq!(s1.error, None);
    // Default state matches new
    assert_eq!(s1.is_visible, s2.is_visible);
    assert_eq!(s1.action, s2.action);
    assert_eq!(s1.source_paths, s2.source_paths);
    assert_eq!(s1.destination_input.text(), s2.destination_input.text());
    assert_eq!(s1.destination_input.cursor(), s2.destination_input.cursor());
    assert_eq!(s1.error, s2.error);
}

#[test]
fn test_reset_state() {
    let mut destination_input = TextEditState::with_text("/baz");
    destination_input.set_cursor(2);
    let mut st = CopyMoveState {
        is_visible: true,
        action: CopyMoveAction::Move,
        source_paths: vec![PathBuf::from("/foo"), PathBuf::from("/bar")],
        destination_input,
        error: Some("err".into()),
    };
    st.reset();
    assert!(!st.is_visible);
    assert_eq!(st.source_paths.len(), 0);
    assert_eq!(st.destination_input.text(), "");
    assert_eq!(st.destination_input.cursor(), 0);
    assert_eq!(st.error, None);
}
