use super::*;

#[test]
fn test_delete_ignores_a_cursor_off_the_visible_listing() {
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    // `.env` is withheld, so it has no row in the visible listing.
    let env = app
        .active_tab()
        .entries
        .iter()
        .position(|e| e.name == ".env")
        .unwrap();
    assert!(!app.active_tab().is_visible(env));

    // A cursor that no longer names a row (as a stale cursor could) must not
    // fall back to acting on an entry that is not on screen.
    app.active_tab_mut().cursor = app.active_tab().visible_count() + 5;

    crate::handlers::popup_delete::handle_init_delete(&mut app, false);
    assert!(
        !app.popups.delete.is_visible,
        "must not offer to delete an entry that is not on screen"
    );
}

#[test]
fn test_copy_ignores_a_cursor_off_the_visible_listing() {
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    app.active_tab_mut().cursor = app.active_tab().visible_count() + 5;

    crate::handlers::popup_copy_move::handle_init_copy(&mut app);
    assert!(
        app.popups.copy_move.source_paths.is_empty(),
        "must not offer to copy an entry that is not on screen"
    );
}

#[test]
fn test_copy_still_works_on_a_visible_entry_with_hidden_hidden() {
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    app.active_tab_mut().cursor = 2; // "README.md", the last visible row

    crate::handlers::popup_copy_move::handle_init_copy(&mut app);
    assert_eq!(app.popups.copy_move.source_paths.len(), 1);
    assert!(app.popups.copy_move.source_paths[0].ends_with("README.md"));
}
