use super::*;

#[test]
fn test_app_new_reconciles_tabs_with_the_saved_setting() {
    // Assemble the app, then flip the setting through the same entry point the
    // toggle uses, and confirm every tab follows.
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    for panel in [&app.panels.left, &app.panels.right] {
        assert_eq!(names(panel.active_tab()), vec!["..", "docs", "README.md"]);
    }
}

#[tokio::test]
async fn test_handle_toggle_hidden_applies_to_every_tab_in_both_panels() {
    let left = manager(tab_with(listing()));
    let mut extra = tab_with(listing());
    extra.current_dir = PathBuf::from("/other");
    let mut left = left;
    left.tabs.push(extra);

    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(left)
        .right(manager(tab_with(listing())))
        .build();

    handle_toggle_hidden(&mut app).await;
    assert!(!app.show_hidden);
    for panel in [&app.panels.left, &app.panels.right] {
        for tab in &panel.tabs {
            assert!(!tab.show_hidden, "every tab follows the global setting");
            assert_eq!(names(tab), vec!["..", "docs", "README.md"]);
        }
    }

    handle_toggle_hidden(&mut app).await;
    assert!(app.show_hidden);
    for panel in [&app.panels.left, &app.panels.right] {
        for tab in &panel.tabs {
            assert!(tab.show_hidden);
            assert_eq!(names(tab).len(), 5);
        }
    }
}

#[tokio::test]
async fn test_toggle_reports_state_in_the_status_line() {
    let mut app = app_with(listing());
    handle_toggle_hidden(&mut app).await;
    let msg = app.active_tab().status_msg.as_ref().unwrap().0.clone();
    assert_eq!(msg, "Hidden files hidden");

    handle_toggle_hidden(&mut app).await;
    let msg = app.active_tab().status_msg.as_ref().unwrap().0.clone();
    assert_eq!(msg, "Hidden files shown");
}

#[tokio::test]
async fn test_toggle_applies_to_a_remote_tab_through_the_app_entry_point() {
    // The same through `handle_toggle_hidden`, which is what the key binding
    // calls: a local and a remote tab in the two panels must both follow.
    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(manager(tab_with(listing())))
        .right(manager(remote_tab_with(remote_listing())))
        .build();

    handle_toggle_hidden(&mut app).await;

    assert!(!app.show_hidden);
    assert_eq!(
        names(app.panels.left.active_tab()),
        vec!["..", "docs", "README.md"]
    );
    assert_eq!(
        names(app.panels.right.active_tab()),
        vec!["..", "docs", "README.md"],
        "a remote tab must honour the same toggle as a local one"
    );
}

#[tokio::test]
async fn test_toggle_refreshes_a_viewer_directory_listing() {
    // The viewer follows the cursor, so its listing is a directory preview. When
    // the hidden setting flips, that preview must reload rather than keep showing
    // the previous rows until the cursor moves.
    let temp = tempfile::tempdir().unwrap();
    let sub = temp.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("visible.txt"), "hi").unwrap();
    std::fs::write(sub.join(".hidden.txt"), "shh").unwrap();
    mark_hidden(&sub.join(".hidden.txt"));

    let mut tab = crate::test_utils::create_test_tab();
    tab.current_dir = temp.path().to_path_buf();
    tab.entries = vec![entry("sub", true, "drwxr-xr-x")];
    tab.cursor = 0;
    tab.recompute_visible_indices();

    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(manager(tab.clone()))
        .right(manager(tab))
        .build();
    app.show_hidden = true;
    app.apply_show_hidden();
    app.file_viewer.is_visible = true;
    crate::handlers::navigation::update_viewer_content(&mut app).await;
    assert!(
        app.file_viewer
            .text
            .content
            .iter()
            .any(|l| l == ".hidden.txt"),
        "precondition: the hidden entry is listed while hidden files are shown"
    );

    handle_toggle_hidden(&mut app).await;
    assert!(!app.show_hidden);
    assert!(
        !app.file_viewer
            .text
            .content
            .iter()
            .any(|l| l == ".hidden.txt"),
        "the listing must follow the toggle, got {:?}",
        app.file_viewer.text.content
    );

    handle_toggle_hidden(&mut app).await;
    assert!(
        app.file_viewer
            .text
            .content
            .iter()
            .any(|l| l == ".hidden.txt"),
        "showing hidden again must bring the entry back"
    );
}
