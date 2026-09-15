use crate::app::AppState;
use crate::handlers::input_utils::handle_text_input;
use termina::event::{KeyCode, Modifiers};

pub fn handle_init_rename_tab(app: &mut AppState) {
    let tab = app.active_tab();
    if tab.is_archive() {
        return;
    }

    let current_title = tab.title().to_string();
    let state = &mut app.popups.rename_tab;
    state.is_visible = true;
    state.new_name.clone_from(&current_title);
    state.cursor_position = current_title.chars().count();
}

pub fn handle_rename_tab_event(code: KeyCode, modifiers: Modifiers, app: &mut AppState) -> bool {
    match code {
        KeyCode::Escape => {
            app.popups.reset_popup(crate::app::PopupKind::RenameTab);
        }
        KeyCode::Enter => {
            let new_name = app.popups.rename_tab.new_name.trim();
            if new_name.is_empty() {
                app.active_tab_mut().custom_title = None;
            } else {
                app.active_tab_mut().custom_title = Some(new_name.to_string());
            }
            app.popups.reset_popup(crate::app::PopupKind::RenameTab);
        }
        _ => {
            handle_text_input(
                code,
                modifiers,
                &mut app.popups.rename_tab.new_name,
                &mut app.popups.rename_tab.cursor_position,
                false,
            );
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::tabs::TabManager;
    use std::path::Path;

    async fn test_app() -> AppState {
        crate::test_utils::TestAppBuilder::new()
            .left(TabManager::new(Path::new(".")).await.unwrap())
            .right(TabManager::new(Path::new(".")).await.unwrap())
            .build()
    }

    #[tokio::test]
    async fn test_init_rename_tab_cursor_starts_at_end_in_chars() {
        let mut app = test_app().await;
        app.panels.left.active_tab_mut().custom_title = Some("héllo ✓".to_string());

        handle_init_rename_tab(&mut app);

        let popup = &app.popups.rename_tab;
        assert!(popup.is_visible);
        assert_eq!(popup.new_name, "héllo ✓");
        // Cursor is a char index: "héllo ✓" is 7 chars but 10 bytes.
        assert_eq!(popup.cursor_position, popup.new_name.chars().count());
        assert!(
            popup.cursor_position <= popup.new_name.chars().count(),
            "cursor position is out of range"
        );
    }

    #[tokio::test]
    async fn test_init_rename_tab_cursor_for_ascii_title() {
        let mut app = test_app().await;
        app.panels.left.active_tab_mut().custom_title = Some("plain".to_string());

        handle_init_rename_tab(&mut app);

        assert_eq!(app.popups.rename_tab.new_name, "plain");
        assert_eq!(app.popups.rename_tab.cursor_position, 5);
    }
}
