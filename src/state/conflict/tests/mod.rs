use super::*;

#[test]
fn show_populates_and_makes_visible() {
    let mut state = ConflictState::new();
    state.focused_button = 3;
    state.show(7, PathBuf::from("/a/b"), ConflictType::FileExists);

    assert!(state.is_visible);
    assert_eq!(state.task_id, 7);
    assert_eq!(state.conflict_path, PathBuf::from("/a/b"));
    assert_eq!(state.focused_button, 0);
}
