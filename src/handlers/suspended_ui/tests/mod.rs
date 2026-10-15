use super::*;

#[test]
fn test_enter_keeps_screen_visible() {
    let mut app = AppState::test_default();
    let suspended = SuspendedUi::enter(&mut app);
    assert!(
        !suspended.screen_cleared,
        "enter() must not clear the screen, graphical editors expect the TUI to stay visible"
    );
}

#[test]
fn test_enter_cleared_clears_screen() {
    let mut app = AppState::test_default();
    let suspended = SuspendedUi::enter_cleared(&mut app);
    assert!(suspended.screen_cleared);
}
