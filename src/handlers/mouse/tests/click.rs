use super::*;

#[tokio::test]
async fn test_mouse_click_on_panel_updates_viewer() {
    let mut app = setup_test_app();

    // Initially path is empty or something default
    assert_eq!(app.file_viewer.path, PathBuf::new());

    // Trigger update for the initial selection (usually happens when viewer is opened)
    crate::handlers::navigation::update_viewer_content(&mut app).await;
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file1.txt"));

    // Simulate mouse click on the second file (file2.txt)
    // area.y = 0, border_offset = 0, header_height = 1 -> content_start_y = 1
    // row 1 is at y = 1 + 1 = 2
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 2,
        modifiers: Modifiers::empty(),
    };

    handle_mouse_event(&mut app, event).await;

    assert_eq!(app.panels.left.active_tab().cursor, 1);
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file2.txt"));
}

/// A click on a directory-listing entry maps the rendered column (icon included)
/// back to the matching character in the entry name.
#[tokio::test]
async fn test_viewer_listing_click_maps_through_icon_offset() {
    let mut app = setup_viewer_listing_app();

    // Row 1 renders as "<icon> alpha.txt"; column 3 is the name's second
    // character, i.e. content index 1.
    handle_mouse_event(
        &mut app,
        mouse_event(MouseEventKind::Down(MouseButton::Left), 3, 1),
    )
    .await;

    assert_eq!(app.file_viewer.text.selection, Some(((1, 1), (1, 1))));
}

/// A double click on a listing entry selects the word under the cursor, again
/// compensating for the icon.
#[tokio::test]
async fn test_viewer_listing_double_click_selects_word_with_icons() {
    let mut app = setup_viewer_listing_app();

    handle_mouse_event(
        &mut app,
        mouse_event(MouseEventKind::Down(MouseButton::Left), 3, 1),
    )
    .await;
    handle_mouse_event(
        &mut app,
        mouse_event(MouseEventKind::Down(MouseButton::Left), 3, 1),
    )
    .await;

    assert_eq!(app.file_viewer.text.selection, Some(((1, 0), (1, 5))));
}

#[tokio::test]
async fn test_mouse_tab_switch_updates_viewer() {
    let mut app = setup_test_app();

    // Add a second tab to the left panel
    let mut tab2 = app.panels.left.tabs[0].clone();
    tab2.current_dir = PathBuf::from("/other");
    tab2.entries = vec![FileEntry {
        name: "other.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(50),
        modified: None,
        attributes: "-rw-r--r--".to_string(),
        selected: false,
    }];
    tab2.recompute_visible_indices();
    app.panels.left.tabs.push(tab2);

    // Set tab areas (simple mock)
    app.layout.left_tab_areas = vec![Rect::new(0, 0, 10, 1), Rect::new(10, 0, 10, 1)];

    crate::handlers::navigation::update_viewer_content(&mut app).await;
    assert_eq!(app.file_viewer.path, PathBuf::from("/test/file1.txt"));

    // Click on the second tab
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 15,
        row: 0,
        modifiers: Modifiers::empty(),
    };

    handle_mouse_event(&mut app, event).await;

    assert_eq!(app.panels.left.active_tab_index, 1);
    assert_eq!(app.file_viewer.path, PathBuf::from("/other/other.txt"));
}

#[tokio::test]
async fn test_empty_trash_mouse_click() {
    let mut app = AppState::test_default();
    let button_areas = empty_trash_button_areas();

    // Open the empty-trash popup and simulate a draw having completed
    app.popups.empty_trash = EmptyTrashState {
        is_visible: false,
        selected_no: true, // "No" is initially focused
    };
    app.layout.popups.empty_trash.button_areas = button_areas.clone();
    app.popups
        .set_popup_visible(crate::app::PopupKind::EmptyTrash, true);

    // The "(Y)es" button is the second one (index 1)
    let yes_btn = button_areas[1];
    // Pick a point inside the Yes button
    let click_x = yes_btn.x + 2;
    let click_y = yes_btn.y + 1;

    // --- Mouse Down on Yes ---
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(
        app.mouse.mouse_button_down_index,
        Some(1),
        "mouse_button_down_index should be Some(1) after clicking Yes"
    );
    assert!(
        !app.popups.empty_trash.selected_no,
        "selected_no should be false after clicking Yes (Yes should be selected)"
    );

    // --- Mouse Up on Yes ---
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert!(
        !app.popups.empty_trash.is_visible,
        "popup should be closed after clicking Yes"
    );
}

#[tokio::test]
async fn test_empty_trash_mouse_click_released_outside() {
    let mut app = AppState::test_default();
    let button_areas = empty_trash_button_areas();

    app.popups.empty_trash = EmptyTrashState {
        is_visible: false,
        selected_no: true,
    };
    app.layout.popups.empty_trash.button_areas = button_areas.clone();
    app.popups
        .set_popup_visible(crate::app::PopupKind::EmptyTrash, true);

    let yes_btn = button_areas[1];
    let click_x = yes_btn.x + 2;
    let click_y = yes_btn.y + 1;

    // Down on Yes
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.mouse_button_down_index, Some(1));

    // Up outside the popup
    let outside_x = 0;
    let outside_y = 0;
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: outside_x,
            row: outside_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert!(
        app.popups.empty_trash.is_visible,
        "popup should stay visible when Up is outside the button"
    );
    // mouse_button_down_index should have been consumed
    assert_eq!(app.mouse.mouse_button_down_index, None);
}

#[tokio::test]
async fn test_empty_trash_mouse_wheel_ignored() {
    let mut app = AppState::test_default();
    app.popups.empty_trash = EmptyTrashState {
        is_visible: false,
        selected_no: true,
    };
    app.layout.popups.empty_trash.button_areas = vec![Rect::new(10, 10, 12, 3)];
    app.popups
        .set_popup_visible(crate::app::PopupKind::EmptyTrash, true);

    let scroll_x = 5u16;
    let scroll_y = 5u16;

    // Scroll events should be silently ignored when popup is visible
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: scroll_x,
            row: scroll_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    // The popup state should be unchanged
    assert!(app.popups.empty_trash.is_visible);
    assert!(app.popups.empty_trash.selected_no);
}

#[tokio::test]
async fn test_ssh_history_remove_confirmation_mouse_click() {
    let mut app = AppState::test_default();

    app.ssh_history
        .connections
        .push(crate::ssh::history::SshConnectionInfo {
            name: Some("server".to_string()),
            connection_string: "user@localhost:22".to_string(),
            user: "user".to_string(),
            host: "localhost".to_string(),
            port: 22,
            path: None,
            sort_column: None,
            sort_direction: None,
        });

    // Open the SSH connection dialog with an active "remove from history"
    // confirmation and simulate a draw pass having populated the button areas.
    app.popups
        .set_popup_visible(crate::app::PopupKind::SshConnection, true);
    app.popups.ssh_connection.confirmation = Some(crate::state::ConfirmationState::new(
        "Remove 'user@localhost:22' from history?".to_string(),
        true,
        crate::state::ConfirmationAction::DeleteSshHistory(0),
    ));
    let confirm_buttons = confirmation_button_areas(66, 6);
    app.layout
        .popups
        .ssh_connection
        .confirmation_buttons
        .button_areas = confirm_buttons.clone();
    let yes_btn = confirm_buttons[1];
    let click_x = yes_btn.x + 2;
    let click_y = yes_btn.y + 1;

    // --- Mouse Down on Yes ---
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.mouse_button_down_index, Some(1));
    assert!(
        !app.popups
            .ssh_connection
            .confirmation
            .as_ref()
            .unwrap()
            .selected_no,
        "Yes should be selected after clicking it"
    );

    // --- Mouse Up on Yes ---
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert!(app.popups.ssh_connection.confirmation.is_none());
    assert!(
        app.ssh_history.connections.is_empty(),
        "history entry should be removed after clicking Yes"
    );
}

#[tokio::test]
async fn test_ssh_history_remove_confirmation_released_outside_keeps_entry() {
    let mut app = AppState::test_default();
    app.ssh_history
        .connections
        .push(crate::ssh::history::SshConnectionInfo {
            name: Some("server".to_string()),
            connection_string: "user@localhost:22".to_string(),
            user: "user".to_string(),
            host: "localhost".to_string(),
            port: 22,
            path: None,
            sort_column: None,
            sort_direction: None,
        });

    app.popups
        .set_popup_visible(crate::app::PopupKind::SshConnection, true);
    app.popups.ssh_connection.confirmation = Some(crate::state::ConfirmationState::new(
        "Remove 'user@localhost:22' from history?".to_string(),
        true,
        crate::state::ConfirmationAction::DeleteSshHistory(0),
    ));
    let confirm_buttons = confirmation_button_areas(66, 6);
    app.layout
        .popups
        .ssh_connection
        .confirmation_buttons
        .button_areas = confirm_buttons.clone();

    let yes_btn = confirm_buttons[1];

    // Down on Yes, up elsewhere: the entry must be kept
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: yes_btn.x + 2,
            row: yes_btn.y + 1,
            modifiers: Modifiers::empty(),
        },
    )
    .await;
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert!(app.popups.ssh_connection.confirmation.is_some());
    assert_eq!(app.ssh_history.connections.len(), 1);
}

#[tokio::test]
async fn test_bookmark_remove_confirmation_mouse_click() {
    let mut app = AppState::test_default();

    app.bookmark_store
        .entries
        .push(crate::bookmarks::BookmarkEntry {
            path: PathBuf::from("/home/user"),
            ssh_user: None,
            ssh_host: None,
            ssh_port: None,
            is_archive: false,
        });

    // Open the bookmark list with an active "remove bookmark" confirmation and
    // simulate a draw pass having populated the button areas.
    app.popups
        .set_popup_visible(crate::app::PopupKind::Bookmark, true);
    app.popups.bookmark.list.is_visible = true;
    app.popups.bookmark.confirmation = Some(crate::state::ConfirmationState::new(
        "Remove bookmark '/home/user'?".to_string(),
        true,
        crate::state::ConfirmationAction::DeleteBookmark(0),
    ));
    let confirm_buttons = confirmation_button_areas(60, 6);
    app.layout.popups.bookmark.confirmation_buttons.button_areas = confirm_buttons.clone();

    let yes_btn = confirm_buttons[1];
    let click_x = yes_btn.x + 2;
    let click_y = yes_btn.y + 1;

    // --- Mouse Down on Yes ---
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert_eq!(app.mouse.mouse_button_down_index, Some(1));
    assert!(
        !app.popups
            .bookmark
            .confirmation
            .as_ref()
            .unwrap()
            .selected_no,
        "Yes should be selected after clicking it"
    );

    // --- Mouse Up on Yes ---
    handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: click_x,
            row: click_y,
            modifiers: Modifiers::empty(),
        },
    )
    .await;

    assert!(app.popups.bookmark.confirmation.is_none());
    assert!(
        app.bookmark_store.entries.is_empty(),
        "bookmark should be removed after clicking Yes"
    );
}
