use super::*;

#[tokio::test]
async fn test_bookmark_add_from_archive_tab_bookmarks_archive_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("bookmarked.zip");
    make_archive(&archive_path, &[("dir/file.txt", b"data")]);

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = archive_tab_app(&archive_path, tx);

    // The tab is inside `dir/file.txt`; only the archive file itself is reachable.
    app.active_tab_mut().current_dir = PathBuf::from("/dir/file.txt");

    handle_bookmark_add(&mut app);

    assert_eq!(app.bookmark_store.entries.len(), 1);
    let entry = &app.bookmark_store.entries[0];
    assert_eq!(entry.path, archive_path);
    assert!(entry.is_archive, "entry should be flagged as an archive");
    assert!(!entry.is_remote());

    // Adding again is a no-op duplicate.
    handle_bookmark_add(&mut app);
    assert_eq!(app.bookmark_store.entries.len(), 1);
}

#[tokio::test]
async fn test_bookmark_add_from_local_tab_is_not_an_archive() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = test_app(Vec::new(), tx, Arc::new(crate::opener::SystemOpener));

    app.active_tab_mut().current_dir = PathBuf::from("/home/user");

    handle_bookmark_add(&mut app);

    assert_eq!(app.bookmark_store.entries.len(), 1);
    let entry = &app.bookmark_store.entries[0];
    assert_eq!(entry.path, PathBuf::from("/home/user"));
    assert!(!entry.is_archive);
}

#[tokio::test]
async fn test_selecting_archive_bookmark_opens_archive_in_new_tab() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("from_bookmark.zip");
    make_archive(&archive_path, &[("hello.txt", b"world")]);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = test_app(Vec::new(), tx, Arc::new(crate::opener::SystemOpener));

    app.bookmark_store.entries.push(BookmarkEntry {
        path: archive_path.clone(),
        ssh_user: None,
        ssh_host: None,
        ssh_port: None,
        is_archive: true,
    });

    app.popups
        .set_popup_visible(crate::app::PopupKind::Bookmark, true);
    refresh_bookmark_list(&mut app);

    handle_bookmark_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    match rx.recv().await {
        Some(UiEvent::Fs(FsEvent::ArchiveLoaded { filename, path, .. })) => {
            assert_eq!(filename, "from_bookmark.zip");
            assert_eq!(path, archive_path);
        }
        other => panic!("Expected ArchiveLoaded event, got {other:?}"),
    }

    assert!(!app.popups.bookmark.list.is_visible);
}

#[tokio::test]
async fn test_selecting_archive_bookmark_works_from_archive_tab() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("other.zip");
    make_archive(&archive_path, &[("hello.txt", b"world")]);

    let (tx, mut rx) = mpsc::unbounded_channel();
    // The active tab is itself an archive, so the old `is_local()` guard on the
    // active tab would wrongly reject the bookmark.
    let mut app = archive_tab_app(&archive_path, tx);

    app.bookmark_store.entries.push(BookmarkEntry {
        path: archive_path.clone(),
        ssh_user: None,
        ssh_host: None,
        ssh_port: None,
        is_archive: true,
    });

    app.popups
        .set_popup_visible(crate::app::PopupKind::Bookmark, true);
    refresh_bookmark_list(&mut app);

    handle_bookmark_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(
        app.active_tab().error.is_none(),
        "unexpected error: {:?}",
        app.active_tab().error
    );

    match rx.recv().await {
        Some(UiEvent::Fs(FsEvent::ArchiveLoaded { filename, path, .. })) => {
            assert_eq!(filename, "other.zip");
            assert_eq!(path, archive_path);
        }
        other => panic!("Expected ArchiveLoaded event, got {other:?}"),
    }
}

#[tokio::test]
async fn test_nested_archive_enter_reports_archive_context_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let outer_path = temp_dir.path().join("outer.zip");
    make_archive(&outer_path, &[("inner.zip", b"PK\x05\x06\0\0")]);

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = archive_tab_app(&outer_path, tx);
    app.active_tab_mut().current_dir = PathBuf::from("/");
    app.active_tab_mut().entries = vec![create_file_entry("inner.zip", false)];
    app.active_tab_mut().cursor = 0;
    app.active_tab_mut().recompute_visible_indices();

    handle_enter(&mut app).await;

    let err = app.active_tab().error.as_deref().unwrap_or("");
    assert!(
        err.contains("inside an archive"),
        "expected an archive-context error, got {err:?}"
    );
    assert!(
        !err.contains("remote"),
        "an archive tab is not a remote connection, got {err:?}"
    );
}

#[tokio::test]
async fn test_selecting_archive_bookmark_does_not_navigate_or_delete() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("kept.zip");
    make_archive(&archive_path, &[("hello.txt", b"world")]);

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = test_app(Vec::new(), tx, Arc::new(crate::opener::SystemOpener));

    app.bookmark_store.entries.push(BookmarkEntry {
        path: archive_path,
        ssh_user: None,
        ssh_host: None,
        ssh_port: None,
        is_archive: true,
    });

    app.popups
        .set_popup_visible(crate::app::PopupKind::Bookmark, true);
    refresh_bookmark_list(&mut app);

    handle_bookmark_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    // The current tab is untouched and the bookmark survives.
    assert_eq!(app.active_tab().current_dir, PathBuf::from("/tmp"));
    assert_eq!(app.bookmark_store.entries.len(), 1);
    assert!(app.active_tab().error.is_none());
}

#[tokio::test]
async fn test_unavailable_archive_bookmark_reports_on_status_line() {
    // A bookmark pointing at an archive that cannot be opened reports the
    // failure on the panel status line; no error popup (whose buttons the
    // archive-open task has no way to act on) is raised.
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("missing.zip");
    make_archive(&archive_path, &[("hello.txt", b"world")]);
    std::fs::remove_file(&archive_path).unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = test_app(Vec::new(), tx, Arc::new(crate::opener::SystemOpener));

    app.bookmark_store.entries.push(BookmarkEntry {
        path: archive_path.clone(),
        ssh_user: None,
        ssh_host: None,
        ssh_port: None,
        is_archive: true,
    });

    app.popups
        .set_popup_visible(crate::app::PopupKind::Bookmark, true);
    refresh_bookmark_list(&mut app);

    handle_bookmark_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for task event")
        .expect("event channel closed");
    assert!(
        matches!(event, UiEvent::Fs(FsEvent::ArchiveOpenFailed { .. })),
        "expected ArchiveOpenFailed, got {event:?}"
    );
    assert!(
        !matches!(event, UiEvent::Alert(_)),
        "an archive open failure must not raise an alert popup"
    );

    // Drain the task's status update too, so the task is actually Failed.
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev, UiEvent::Task(_)) {
            crate::handlers::event_dispatch::dispatch_ui_event(ev, &mut app).await;
        }
    }
    crate::handlers::event_dispatch::dispatch_ui_event(event, &mut app).await;

    assert!(
        !app.popups.error.is_visible,
        "the error popup should not be shown"
    );
    let err = app.active_tab().error.as_deref().unwrap_or("");
    assert!(
        err.contains("missing.zip"),
        "status line should name the archive, got {err:?}"
    );

    // The failure is on the active tab's status line only; the task result line
    // must not repeat it.
    let tasks = app.tasks.task_manager.get_tasks();
    let task = tasks.last().expect("the open task should still be listed");
    assert!(
        task.error_reported,
        "the failure is already on a status line, so the task result line must skip it"
    );
    assert!(
        matches!(task.status, crate::tasks::TaskStatus::Failed(_)),
        "the task itself should still be marked failed, got {:?}",
        task.status
    );
}
