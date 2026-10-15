use super::*;

#[test]
fn test_new_and_default() {
    let state = ErrorState::new();
    assert!(!state.is_visible);
    assert_eq!(state.task_id, 0);
    assert_eq!(state.error_path, "");
    assert_eq!(state.error_message, "");

    let default_state = ErrorState::default();
    assert_eq!(state.is_visible, default_state.is_visible);
    assert_eq!(state.task_id, default_state.task_id);
    assert_eq!(state.error_path, default_state.error_path);
    assert_eq!(state.error_message, default_state.error_message);
}

#[test]
fn test_show_populates_and_makes_visible() {
    let mut state = ErrorState::new();
    state.focused_button = 2;
    state.show(5, "some/path".to_string(), "boom".to_string());

    assert!(state.is_visible);
    assert_eq!(state.task_id, 5);
    assert_eq!(state.error_path, "some/path");
    assert_eq!(state.error_message, "boom");
    assert_eq!(state.focused_button, 0);
}

#[test]
fn test_reset() {
    let mut state = ErrorState {
        is_visible: true,
        task_id: 123,
        error_path: String::from("some/path"),
        error_message: String::from("error occurred"),
        focused_button: 0,
    };
    state.reset();
    assert!(!state.is_visible);
    assert_eq!(state.task_id, 0);
    assert_eq!(state.error_path, "");
    assert_eq!(state.error_message, "");
}
