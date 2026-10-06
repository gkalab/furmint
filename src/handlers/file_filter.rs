use crate::app::AppState;
use crate::handlers::input_utils::handle_text_input;
use termina::event::{KeyCode, Modifiers};

pub fn handle_init_file_filter(app: &mut AppState) {
    app.active_tab_mut().init_file_filter();
}

pub fn handle_file_filter_event(code: KeyCode, modifiers: Modifiers, app: &mut AppState) -> bool {
    match code {
        KeyCode::Escape => {
            app.active_tab_mut().cancel_file_filter();
        }
        KeyCode::Enter => {
            app.active_tab_mut().confirm_file_filter();
        }
        _ => {
            let filter = &mut app.active_tab_mut().filter;
            let changed = handle_text_input(
                code,
                modifiers,
                &mut filter.pattern,
                &mut filter.cursor_position,
                false,
            );
            if changed {
                app.active_tab_mut().apply_file_filter();
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_app;

    #[test]
    fn test_handle_init_file_filter() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);
        let tab = app.active_tab();
        assert!(tab.filter.active);
        assert_eq!(tab.filter.pattern, "");
    }

    #[test]
    fn test_handle_file_filter_event_types_char() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Type 'f'
        handle_file_filter_event(KeyCode::Char('f'), Modifiers::NONE, &mut app);
        let tab = app.active_tab();
        assert_eq!(tab.filter.pattern, "f");
        assert!(tab.filter.active);
    }

    #[test]
    fn test_handle_file_filter_event_invalid_glob() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Type an invalid glob pattern (unclosed character class)
        handle_file_filter_event(KeyCode::Char('['), Modifiers::NONE, &mut app);
        let tab = app.active_tab();
        assert_eq!(tab.filter.pattern, "[");
        assert!(tab.filter.active);

        // The filter should still be active (not applied due to invalid glob)
        assert!(!tab.filter.is_active());
    }

    #[test]
    fn test_handle_file_filter_event_valid_glob() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Type a valid glob pattern
        handle_file_filter_event(KeyCode::Char('f'), Modifiers::NONE, &mut app);
        handle_file_filter_event(KeyCode::Char('o'), Modifiers::NONE, &mut app);
        handle_file_filter_event(KeyCode::Char('o'), Modifiers::NONE, &mut app);
        let tab = app.active_tab();
        assert_eq!(tab.filter.pattern, "foo");
        assert!(tab.filter.active);
        // Valid glob should be applied
        assert!(tab.filter.is_active());
    }

    #[test]
    fn test_handle_file_filter_event_escape_cancels() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Type something
        handle_file_filter_event(KeyCode::Char('f'), Modifiers::NONE, &mut app);

        // Press Escape
        handle_file_filter_event(KeyCode::Escape, Modifiers::NONE, &mut app);
        let tab = app.active_tab();
        assert!(!tab.filter.active);
        assert_eq!(tab.filter.pattern, "");
    }

    #[test]
    fn test_handle_file_filter_event_confirm_valid() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Type a valid glob pattern
        handle_file_filter_event(KeyCode::Char('f'), Modifiers::NONE, &mut app);
        handle_file_filter_event(KeyCode::Char('o'), Modifiers::NONE, &mut app);
        handle_file_filter_event(KeyCode::Char('o'), Modifiers::NONE, &mut app);

        // Confirm with Enter
        handle_file_filter_event(KeyCode::Enter, Modifiers::NONE, &mut app);
        let tab = app.active_tab();
        assert!(!tab.filter.active);
        assert_eq!(tab.filter.pattern, "");
        assert!(tab.filter.is_active());
    }

    #[test]
    fn test_handle_file_filter_event_confirm_invalid_keeps_active() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Type an invalid glob pattern
        handle_file_filter_event(KeyCode::Char('['), Modifiers::NONE, &mut app);

        // Confirm with Enter - should NOT apply invalid glob
        handle_file_filter_event(KeyCode::Enter, Modifiers::NONE, &mut app);
        let tab = app.active_tab();
        // Filter should still be active so user can fix the pattern
        assert!(tab.filter.active);
        assert_eq!(tab.filter.pattern, "[");
        // Should NOT be applied
        assert!(!tab.filter.is_active());
    }

    #[test]
    fn test_handle_file_filter_event_confirm_empty_clears() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Apply a valid filter first
        let _ = app.active_tab_mut().set_file_filter(Some("foo"));

        // Init filter mode
        handle_init_file_filter(&mut app);

        // Confirm with empty pattern
        handle_file_filter_event(KeyCode::Enter, Modifiers::NONE, &mut app);
        let tab = app.active_tab();
        assert!(!tab.filter.active);
        assert_eq!(tab.filter.pattern, "");
        // Filter should be cleared
        assert!(!tab.filter.is_active());
    }

    #[test]
    fn test_confirm_file_filter_with_invalid_glob() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Type invalid glob
        handle_file_filter_event(KeyCode::Char('['), Modifiers::NONE, &mut app);

        // Confirm - should keep filter active
        app.active_tab_mut().confirm_file_filter();
        let tab = app.active_tab();
        assert!(tab.filter.active);
        assert_eq!(tab.filter.pattern, "[");
        assert!(!tab.filter.is_active());
    }

    #[test]
    fn test_confirm_file_filter_with_valid_glob() {
        let mut app = create_test_app();
        handle_init_file_filter(&mut app);

        // Type valid glob
        handle_file_filter_event(KeyCode::Char('a'), Modifiers::NONE, &mut app);

        // Confirm - should apply filter
        app.active_tab_mut().confirm_file_filter();
        let tab = app.active_tab();
        assert!(!tab.filter.active);
        assert_eq!(tab.filter.pattern, "");
        assert!(tab.filter.is_active());
    }
}
