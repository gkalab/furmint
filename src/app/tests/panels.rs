use super::*;

#[tokio::test]
async fn test_new_tab_inherits_sort() {
    let mut manager = TabManager::new(&std::env::temp_dir()).await.unwrap();

    // Change sort on active tab
    manager.active_tab_mut().sort.column = SortColumn::Size;
    manager.active_tab_mut().sort.direction = SortDirection::Descending;

    // Create new tab
    manager.new_tab(&std::env::temp_dir(), None).await.unwrap();

    // Check new tab (which is now active)
    let new_tab = manager.active_tab();
    assert_eq!(new_tab.sort.column, SortColumn::Size);
    assert_eq!(new_tab.sort.direction, SortDirection::Descending);
}

#[tokio::test]
async fn test_can_swap_active_tabs() {
    let local_tab = create_test_tab();
    let remote_tab = remote_tab("remote");

    let mut app = AppState::test_default();
    app.panels.left.tabs = vec![local_tab.clone()];
    app.panels.right.tabs = vec![remote_tab.clone()];

    // Case 1: Left has 1 local, Right has 0 local. Swapping Left (local) with Right (remote) would leave Left with 0 local.
    assert!(app.can_swap_active_tabs().is_err());

    // Case 2: Add another local tab to Left
    app.panels.left.tabs.push(local_tab.clone());
    assert!(app.can_swap_active_tabs().is_ok());

    // Case 3: Swap allowed when both are local
    app.panels.right.tabs[0] = local_tab.clone();
    assert!(app.can_swap_active_tabs().is_ok());
}

#[tokio::test]
#[cfg(windows)]
#[allow(clippy::too_many_lines)]
async fn test_drive_navigation_matches_opposite_pane() {
    let provider = Arc::new(MockProvider::local());

    // Test Case 1: Switching to drive D: on Left (active) while Right is on D:\RightDir.
    // Since selected drive is different from Left's C:\LeftDir but same as Right's drive,
    // Left should navigate to D:\RightDir.
    {
        let mut app = AppState::test_default();
        let left_tab = Tab {
            provider: provider.clone(),
            current_dir: PathBuf::from("C:\\LeftDir"),
            entries: vec![],
            cursor: 0,
            search: IncrementalSearch::default(),
            sort: SortSettings::default(),
            scroll_offset: 0,
            error: None,
            custom_title: None,
            ssh_session_id: None,
            status_msg: None,
            dir_sizes: std::collections::HashMap::new(),
            is_reloading: false,
            visible_indices: Vec::new(),
            filter: crate::state::FileFilterState::new(),
            show_hidden: true,
        };
        let right_tab = Tab {
            provider: provider.clone(),
            current_dir: PathBuf::from("D:\\RightDir"),
            entries: vec![],
            cursor: 0,
            search: IncrementalSearch::default(),
            sort: SortSettings::default(),
            scroll_offset: 0,
            error: None,
            custom_title: None,
            ssh_session_id: None,
            status_msg: None,
            dir_sizes: std::collections::HashMap::new(),
            is_reloading: false,
            visible_indices: Vec::new(),
            filter: crate::state::FileFilterState::new(),
            show_hidden: true,
        };
        app.panels.left.tabs = vec![left_tab];
        app.panels.right.tabs = vec![right_tab];
        app.panels.active = PanelSide::Left;

        app.popups
            .set_popup_visible(crate::app::PopupKind::DriveSelect, true);
        app.popups.drive_select.drives = vec![
            Drive {
                path: "C:\\".to_string(),
                detail: "C:\\".to_string(),
                detail_short: String::new(),
            },
            Drive {
                path: "D:\\".to_string(),
                detail: "D:\\".to_string(),
                detail_short: String::new(),
            },
        ];
        app.popups.drive_select.side = PanelSide::Left;
        app.popups.drive_select.selected_index = 1; // "D:\"

        crate::handlers::popup_drive_select::handle_drive_select_event(
            termina::event::KeyCode::Enter,
            &mut app,
        )
        .await;

        assert_eq!(
            app.panels.left.active_tab().current_dir,
            PathBuf::from("D:\\RightDir")
        );
    }

    // Test Case 2: Selecting a drive that does not match the opposite pane.
    // Switching to drive D: on Left (active) while Right is on E:\RightDir.
    // Left should navigate to D:\.
    {
        let mut app = AppState::test_default();
        let left_tab = Tab {
            provider: provider.clone(),
            current_dir: PathBuf::from("C:\\LeftDir"),
            entries: vec![],
            cursor: 0,
            search: IncrementalSearch::default(),
            sort: SortSettings::default(),
            scroll_offset: 0,
            error: None,
            custom_title: None,
            ssh_session_id: None,
            status_msg: None,
            dir_sizes: std::collections::HashMap::new(),
            is_reloading: false,
            visible_indices: Vec::new(),
            filter: crate::state::FileFilterState::new(),
            show_hidden: true,
        };
        let right_tab = Tab {
            provider: provider.clone(),
            current_dir: PathBuf::from("E:\\RightDir"),
            entries: vec![],
            cursor: 0,
            search: IncrementalSearch::default(),
            sort: SortSettings::default(),
            scroll_offset: 0,
            error: None,
            custom_title: None,
            ssh_session_id: None,
            status_msg: None,
            dir_sizes: std::collections::HashMap::new(),
            is_reloading: false,
            visible_indices: Vec::new(),
            filter: crate::state::FileFilterState::new(),
            show_hidden: true,
        };
        app.panels.left.tabs = vec![left_tab];
        app.panels.right.tabs = vec![right_tab];
        app.panels.active = PanelSide::Left;

        app.popups
            .set_popup_visible(crate::app::PopupKind::DriveSelect, true);
        app.popups.drive_select.drives = vec![
            Drive {
                path: "C:\\".to_string(),
                detail: "C:\\".to_string(),
                detail_short: String::new(),
            },
            Drive {
                path: "D:\\".to_string(),
                detail: "D:\\".to_string(),
                detail_short: String::new(),
            },
        ];
        app.popups.drive_select.side = PanelSide::Left;
        app.popups.drive_select.selected_index = 1; // "D:\"

        crate::handlers::popup_drive_select::handle_drive_select_event(
            termina::event::KeyCode::Enter,
            &mut app,
        )
        .await;

        assert_eq!(
            app.panels.left.active_tab().current_dir,
            PathBuf::from("D:\\")
        );
    }

    // Test Case 3: Selecting the same drive as active pane.
    // Left pane is on C:\LeftDir, Right pane is on C:\RightDir.
    // Switch Left to drive C:\. It is the same drive, but matches the opposite pane, so it should go to C:\RightDir.
    {
        let mut app = AppState::test_default();
        let left_tab = Tab {
            provider: provider.clone(),
            current_dir: PathBuf::from("C:\\LeftDir"),
            entries: vec![],
            cursor: 0,
            search: IncrementalSearch::default(),
            sort: SortSettings::default(),
            scroll_offset: 0,
            error: None,
            custom_title: None,
            ssh_session_id: None,
            status_msg: None,
            dir_sizes: std::collections::HashMap::new(),
            is_reloading: false,
            visible_indices: Vec::new(),
            filter: crate::state::FileFilterState::new(),
            show_hidden: true,
        };
        let right_tab = Tab {
            provider: provider.clone(),
            current_dir: PathBuf::from("C:\\RightDir"),
            entries: vec![],
            cursor: 0,
            search: IncrementalSearch::default(),
            sort: SortSettings::default(),
            scroll_offset: 0,
            error: None,
            custom_title: None,
            ssh_session_id: None,
            status_msg: None,
            dir_sizes: std::collections::HashMap::new(),
            is_reloading: false,
            visible_indices: Vec::new(),
            filter: crate::state::FileFilterState::new(),
            show_hidden: true,
        };
        app.panels.left.tabs = vec![left_tab];
        app.panels.right.tabs = vec![right_tab];
        app.panels.active = PanelSide::Left;

        app.popups
            .set_popup_visible(crate::app::PopupKind::DriveSelect, true);
        app.popups.drive_select.drives = vec![
            Drive {
                path: "C:\\".to_string(),
                detail: "C:\\".to_string(),
                detail_short: String::new(),
            },
            Drive {
                path: "D:\\".to_string(),
                detail: "D:\\".to_string(),
                detail_short: String::new(),
            },
        ];
        app.popups.drive_select.side = PanelSide::Left;
        app.popups.drive_select.selected_index = 0; // "C:\"

        crate::handlers::popup_drive_select::handle_drive_select_event(
            termina::event::KeyCode::Enter,
            &mut app,
        )
        .await;

        assert_eq!(
            app.panels.left.active_tab().current_dir,
            PathBuf::from("C:\\RightDir")
        );
    }
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn test_drive_navigation_uses_exact_mount_path() {
    let provider = Arc::new(MockProvider::local());

    let local_tab = |dir: &str| Tab {
        provider: provider.clone(),
        current_dir: PathBuf::from(dir),
        entries: vec![],
        cursor: 0,
        search: IncrementalSearch::default(),
        sort: SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        ssh_session_id: None,
        status_msg: None,
        dir_sizes: std::collections::HashMap::new(),
        is_reloading: false,
        visible_indices: Vec::new(),
        filter: crate::state::FileFilterState::new(),
        show_hidden: true,
    };

    // Test Case 1: Selecting /mnt on Left (active) while Right is on /mnt/data.
    // The opposite pane lives inside the selected mount, but on Linux the exact
    // mount path is used, so Left navigates to the mount root.
    {
        let mut app = AppState::test_default();
        app.panels.left.tabs = vec![local_tab("/home/user/proj")];
        app.panels.right.tabs = vec![local_tab("/mnt/data")];
        app.panels.active = PanelSide::Left;

        app.popups
            .set_popup_visible(crate::app::PopupKind::DriveSelect, true);
        app.popups.drive_select.drives = vec![
            Drive {
                path: "/".to_string(),
                detail: "/".to_string(),
                detail_short: String::new(),
            },
            Drive {
                path: "/mnt".to_string(),
                detail: "/mnt".to_string(),
                detail_short: String::new(),
            },
        ];
        app.popups.drive_select.side = PanelSide::Left;
        app.popups.drive_select.selected_index = 1; // "/mnt"

        crate::handlers::popup_drive_select::handle_drive_select_event(
            termina::event::KeyCode::Enter,
            &mut app,
        )
        .await;

        assert_eq!(
            app.panels.left.active_tab().current_dir,
            PathBuf::from("/mnt")
        );
    }

    // Test Case 2: Selecting /mnt on Left (active) while Right is on /etc.
    // The opposite pane is outside the selected mount, so Left navigates
    // to the mount root.
    {
        let mut app = AppState::test_default();
        app.panels.left.tabs = vec![local_tab("/home/user/proj")];
        app.panels.right.tabs = vec![local_tab("/etc")];
        app.panels.active = PanelSide::Left;

        app.popups
            .set_popup_visible(crate::app::PopupKind::DriveSelect, true);
        app.popups.drive_select.drives = vec![
            Drive {
                path: "/".to_string(),
                detail: "/".to_string(),
                detail_short: String::new(),
            },
            Drive {
                path: "/mnt".to_string(),
                detail: "/mnt".to_string(),
                detail_short: String::new(),
            },
        ];
        app.popups.drive_select.side = PanelSide::Left;
        app.popups.drive_select.selected_index = 1; // "/mnt"

        crate::handlers::popup_drive_select::handle_drive_select_event(
            termina::event::KeyCode::Enter,
            &mut app,
        )
        .await;

        assert_eq!(
            app.panels.left.active_tab().current_dir,
            PathBuf::from("/mnt")
        );
    }
}

#[tokio::test]
async fn test_move_active_tab_to_other_side() {
    let mut app = AppState::test_default();

    assert_eq!(app.panels.left.tabs.len(), 1);
    assert_eq!(app.panels.right.tabs.len(), 1);

    // 1. Moving the only tab should fail.
    app.panels.active = PanelSide::Left;
    let res = app.move_active_tab_to_other_side(PanelSide::Right);
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err(),
        "Cannot move the last local tab to the other side"
    );

    // Add another local tab to Left.
    let tab_clone = app.panels.left.tabs[0].clone();
    app.panels.left.tabs.push(tab_clone);
    assert_eq!(app.panels.left.tabs.len(), 2);
    assert_eq!(app.panels.left.local_tab_count(), 2);

    // Set a title on the tab we want to move.
    app.panels.left.tabs[0].custom_title = Some("MovedTab".to_string());
    app.panels.left.active_tab_index = 0;

    // 2. Now move it to Right.
    let res = app.move_active_tab_to_other_side(PanelSide::Right);
    assert!(res.is_ok());

    // Active panel should now be Right.
    assert_eq!(app.panels.active, PanelSide::Right);

    // Left should now have 1 tab.
    assert_eq!(app.panels.left.tabs.len(), 1);

    // Right should now have 2 tabs.
    assert_eq!(app.panels.right.tabs.len(), 2);

    // The active tab on Right should be the one we moved (custom_title: "MovedTab").
    assert_eq!(
        app.panels.right.active_tab().custom_title.as_deref(),
        Some("MovedTab")
    );
    assert_eq!(app.panels.right.active_tab_index, 1);

    // 3. Move it back to Left.
    let res = app.move_active_tab_to_other_side(PanelSide::Left);
    assert!(res.is_ok());
    assert_eq!(app.panels.active, PanelSide::Left);
    assert_eq!(app.panels.left.tabs.len(), 2);
    assert_eq!(app.panels.right.tabs.len(), 1);
    assert_eq!(
        app.panels.left.active_tab().custom_title.as_deref(),
        Some("MovedTab")
    );
    assert_eq!(app.panels.left.active_tab_index, 1);
}
