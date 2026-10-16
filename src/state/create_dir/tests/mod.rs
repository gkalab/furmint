use super::*;
use crate::ui::text_edit::TextEditState;

#[test]
fn test_new_and_default() {
    let s1 = CreateDirectoryState::new();
    let s2 = CreateDirectoryState::default();
    assert!(!s1.is_visible);
    assert_eq!(s1.new_name.text(), "");
    assert_eq!(s1.new_name.cursor(), 0);
    assert_eq!(s1.error, None);
    // Default state matches new
    assert_eq!(s1.is_visible, s2.is_visible);
    assert_eq!(s1.new_name.text(), s2.new_name.text());
    assert_eq!(s1.new_name.cursor(), s2.new_name.cursor());
    assert_eq!(s1.error, s2.error);
}

#[test]
fn test_reset() {
    let mut new_name = TextEditState::with_text("testdir");
    new_name.set_cursor(3);
    let mut st = CreateDirectoryState {
        is_visible: true,
        new_name,
        error: Some("err".into()),
    };
    st.reset();
    assert!(!st.is_visible);
    assert_eq!(st.new_name.text(), "");
    assert_eq!(st.new_name.cursor(), 0);
    assert_eq!(st.error, None);
}
