use super::*;

#[test]
fn dismissed_menu_maps_to_no_verb() {
    assert_eq!(
        menu_result_to_verb_offset(0, CMD_ID_FIRST, CMD_ID_LAST),
        None
    );
}

#[test]
fn invalid_track_popup_menu_results_map_to_no_verb() {
    // Regression: the old `(cmd - 1) as usize` expression wrapped for a
    // negative `TrackPopupMenu` result, and the wrapped value was cast
    // straight to a `lpVerb` pointer (undefined behavior).
    assert_eq!(
        menu_result_to_verb_offset(-1, CMD_ID_FIRST, CMD_ID_LAST),
        None
    );
    assert_eq!(
        menu_result_to_verb_offset(i32::MIN, CMD_ID_FIRST, CMD_ID_LAST),
        None
    );
    assert_eq!(
        menu_result_to_verb_offset(CMD_ID_LAST + 1, CMD_ID_FIRST, CMD_ID_LAST),
        None,
    );
}

#[test]
fn valid_command_ids_map_to_zero_based_offsets() {
    assert_eq!(
        menu_result_to_verb_offset(1, CMD_ID_FIRST, CMD_ID_LAST),
        Some(0)
    );
    assert_eq!(
        menu_result_to_verb_offset(5, CMD_ID_FIRST, CMD_ID_LAST),
        Some(4)
    );
    assert_eq!(
        menu_result_to_verb_offset(CMD_ID_LAST, CMD_ID_FIRST, CMD_ID_LAST),
        Some(CMD_ID_LAST as usize - 1),
    );
    // Non-default first ID.
    assert_eq!(menu_result_to_verb_offset(10, 10, 0x7FFF), Some(0));
    assert_eq!(menu_result_to_verb_offset(12, 10, 0x7FFF), Some(2));
}

#[test]
fn scan_ignores_hidden_windows_on_this_thread() {
    unsafe {
        let hidden = create_dummy_window().expect("create hidden window");
        assert!(find_thread_dialog().is_none());
        let _ = DestroyWindow(hidden);
    }
}

#[test]
fn scan_detects_visible_thread_windows_structurally() {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, WINDOW_EX_STYLE, WS_POPUP, WS_VISIBLE,
        };

        let visible = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            windows::core::w!("STATIC"),
            windows::core::w!(""),
            WS_POPUP | WS_VISIBLE,
            -10_000,
            -10_000,
            1,
            1,
            None,
            None,
            None,
            None,
        )
        .expect("create visible window");

        // No localized-string matching: a visible top-level window owned
        // by this thread is detected by ownership and visibility alone.
        assert_eq!(find_thread_dialog(), Some(visible));

        let _ = DestroyWindow(visible);
        assert!(find_thread_dialog().is_none());
    }
}
