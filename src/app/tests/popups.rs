use super::*;

#[tokio::test]
async fn test_create_file_state_reset() {
    let mut s = CreateFileState::new();
    s.input_value = TextEditState::with_text("test");
    s.input_value.set_cursor(2);
    s.is_visible = true;
    s.error = Some("nope".to_string());
    s.reset();
    assert!(!s.is_visible);
    assert_eq!(s.input_value.text(), "");
    assert_eq!(s.input_value.cursor(), 0);
    assert!(s.error.is_none());
    assert_eq!(s.parent_dir, std::path::PathBuf::new());
}

#[tokio::test]
async fn test_help_state_reset() {
    let mut s = HelpState::new();
    s.is_visible = true;
    s.reset();
    assert!(!s.is_visible);
}

/// The popup table is the single source of truth: every `PopupKind` must flip
/// exactly one real flag and be reflected by `any_visible`. A popup wired into
/// the enum but not the table would break this.
#[test]
fn popup_visibility_matches_any_visible_for_every_kind() {
    let kinds: Vec<PopupKind> = PopupKind::all().collect();
    assert_eq!(kinds.len(), 18);

    for kind in kinds {
        let mut popups = crate::app::Popups::new();
        assert!(!popups.any_visible(), "{kind:?} should start hidden");

        popups.set_popup_visible(kind, true);
        assert!(kind.is_visible(&popups), "{kind:?} did not become visible");
        assert!(
            popups.any_visible(),
            "{kind:?} is visible but any_visible() says otherwise"
        );

        popups.set_popup_visible(kind, false);
        assert!(!kind.is_visible(&popups), "{kind:?} did not become hidden");
        assert!(
            !popups.any_visible(),
            "{kind:?} is hidden but any_visible() says otherwise"
        );
    }
}

/// `reset_popup` must hide the popup regardless of how many were open, and must
/// leave the rest untouched.
#[test]
fn reset_popup_only_clears_the_requested_kind() {
    for kind in PopupKind::all() {
        let mut popups = crate::app::Popups::new();
        popups.set_popup_visible(kind, true);

        popups.reset_popup(kind);

        assert!(!kind.is_visible(&popups), "{kind:?} still visible");
        assert!(!popups.any_visible(), "{kind:?} survived reset_popup");
    }
}

/// Routes a key press to the first visible popup in registry order.
#[tokio::test]
async fn route_popup_key_dispatches_to_the_visible_popup() {
    let mut app = AppState::test_default();
    assert_eq!(
        app.route_popup_key(KeyCode::Escape, Modifiers::NONE).await,
        None,
        "no popup is open, so nothing should be routed"
    );

    app.popups.set_popup_visible(PopupKind::Help, true);
    let handled = app.route_popup_key(KeyCode::Escape, Modifiers::NONE).await;
    assert_eq!(handled, Some(false));
    assert!(
        !app.popups.help.is_visible,
        "Escape should have closed help"
    );
}

/// When two registered popups are open, the first row of `declare_popups!`
/// wins — Error precedes Help, so Escape reaches Error and leaves Help open.
#[tokio::test]
async fn route_popup_key_prefers_the_higher_priority_popup() {
    let mut app = AppState::test_default();
    app.popups.set_popup_visible(PopupKind::Error, true);
    app.popups.set_popup_visible(PopupKind::Help, true);

    app.route_popup_key(KeyCode::Escape, Modifiers::NONE).await;

    assert!(!app.popups.error.is_visible, "Error is routed first");
    assert!(
        app.popups.help.is_visible,
        "Help must not have been reached"
    );
}
