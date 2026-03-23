#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};
    use fm::handlers::popup_rename_tab::{handle_init_rename_tab, handle_rename_tab_event};
    use fm::test_utils::create_test_app;

    #[test]
    fn test_init_rename_tab_opens_popup() {
        let mut app = create_test_app();
        handle_init_rename_tab(&mut app);
        assert!(app.popups.rename_tab.is_visible);
        assert_eq!(app.popups.rename_tab.new_name, app.active_tab().title());
        assert_eq!(
            app.popups.rename_tab.cursor_position,
            app.popups.rename_tab.new_name.len()
        );
    }

    #[test]
    fn test_rename_tab_esc_resets() {
        let mut app = create_test_app();
        handle_init_rename_tab(&mut app);
        handle_rename_tab_event(KeyCode::Esc, KeyModifiers::NONE, &mut app);
        assert!(!app.popups.rename_tab.is_visible);
    }

    #[test]
    fn test_rename_tab_enter_applies_name() {
        let mut app = create_test_app();
        handle_init_rename_tab(&mut app);
        app.popups.rename_tab.new_name = "New Title".to_string();
        handle_rename_tab_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);
        assert!(!app.popups.rename_tab.is_visible);
        assert_eq!(app.active_tab().custom_title, Some("New Title".to_string()));
        assert_eq!(app.active_tab().title(), "New Title");
    }

    #[test]
    fn test_rename_tab_enter_empty_clears_title() {
        let mut app = create_test_app();
        app.active_tab_mut().custom_title = Some("Old Title".to_string());
        handle_init_rename_tab(&mut app);
        app.popups.rename_tab.new_name = "   ".to_string();
        handle_rename_tab_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);
        assert!(!app.popups.rename_tab.is_visible);
        assert_eq!(app.active_tab().custom_title, None);
    }
}
