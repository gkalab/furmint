use super::*;

#[tokio::test]
async fn test_handle_copy_move_validation_same_path() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let file_path = temp_dir.join("test_file_copy_val.txt");
    std::fs::File::create(&file_path).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups
        .set_popup_visible(crate::app::PopupKind::CopyMove, true);
    app.popups.copy_move.source_paths = vec![file_path.clone()];
    app.popups.copy_move.destination_input =
        TextEditState::with_text(temp_dir.to_string_lossy().to_string());

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(app.popups.copy_move.error.is_some());
    assert!(
        app.popups
            .copy_move
            .error
            .as_ref()
            .unwrap()
            .contains("same")
    );

    std::fs::remove_file(&file_path).ok();
}

#[tokio::test]
async fn test_handle_copy_move_validation_into_itself() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let src_dir = temp_dir.join("test_src_dir");
    std::fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = src_dir.join("test_dest_dir");

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups
        .set_popup_visible(crate::app::PopupKind::CopyMove, true);
    app.popups.copy_move.source_paths = vec![src_dir.clone()];
    app.popups.copy_move.destination_input =
        TextEditState::with_text(dest_dir.to_string_lossy().to_string());

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(app.popups.copy_move.error.is_some());
    assert!(
        app.popups
            .copy_move
            .error
            .as_ref()
            .unwrap()
            .contains("subdirectory of itself")
    );

    std::fs::remove_dir_all(&src_dir).ok();
}

#[tokio::test]
async fn test_handle_paste_validation_same_path() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let file_path = temp_dir.join("test_file_paste_val.txt");
    std::fs::File::create(&file_path).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.panels.left.active_tab_mut().current_dir = temp_dir.clone();

    let data = FileClipboardData {
        action: FileClipboardAction::Copy,
        paths: vec![file_path.clone()],
        source_provider: Arc::new(LocalFs::new()),
    };
    app.os.clipboard.set(data).unwrap();

    handle_paste(&mut app).await;

    assert!(app.panels.left.active_tab().error.is_some());
    assert!(
        app.panels
            .left
            .active_tab()
            .error
            .as_ref()
            .unwrap()
            .contains("same")
    );

    std::fs::remove_file(&file_path).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn test_copy_refuses_a_read_only_destination_before_writing() {
    use crate::tasks::{TaskEvent, TaskStatus, UiEvent};
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("file.txt");
    std::fs::write(&src_file, b"hello").unwrap();

    let dest_dir = tmp.path().join("ro");
    std::fs::create_dir(&dest_dir).unwrap();
    std::fs::set_permissions(&dest_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let (task_tx, mut task_rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut app = crate::test_utils::TestAppBuilder::new()
        .task_tx(task_tx)
        .build();

    crate::handlers::popup_copy_move::spawn_copy_move_task(
        &mut app,
        Arc::new(LocalFs::new()),
        Arc::new(LocalFs::new()),
        vec![src_file],
        dest_dir.to_string_lossy().to_string(),
        CopyMoveAction::Copy,
    );

    let status = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match task_rx.recv().await {
                Some(UiEvent::Task(TaskEvent::UpdateStatus { status, .. })) => break status,
                None => panic!("the task channel closed before a status arrived"),
                Some(_) => {}
            }
        }
    })
    .await
    .expect("the pre-flight must settle the task without hanging");

    let TaskStatus::Failed(msg) = status else {
        panic!("a read-only destination must fail the task, got {status:?}");
    };
    assert!(
        msg.contains("not writable"),
        "the reason must name it: {msg}"
    );
    assert!(
        !dest_dir.join("file.txt").exists(),
        "nothing may be written to a read-only destination"
    );

    std::fs::set_permissions(&dest_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The task status must name the reason a transfer ended: an incomplete move's
/// message is surfaced instead of collapsing into a bare count, and a partial
/// failure is distinguished from a total one.
#[tokio::test]
async fn test_report_transfer_status_surfaces_the_reason() {
    use crate::tasks::{EventBus, TaskEvent, TaskStatus, UiEvent};
    use tokio::sync::mpsc::UnboundedReceiver;

    async fn next_status(rx: &mut UnboundedReceiver<UiEvent>) -> TaskStatus {
        match rx.recv().await {
            Some(UiEvent::Task(TaskEvent::UpdateStatus { status, .. })) => status,
            other => panic!("expected a status update, got {other:?}"),
        }
    }

    let (raw_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let tx = EventBus::new(raw_tx);

    crate::handlers::popup_copy_move::report_transfer_status(&tx, 1, false, &[], 1);
    assert_eq!(next_status(&mut rx).await, TaskStatus::Completed);

    crate::handlers::popup_copy_move::report_transfer_status(&tx, 2, true, &[], 1);
    assert_eq!(next_status(&mut rx).await, TaskStatus::Cancelled);

    crate::handlers::popup_copy_move::report_transfer_status(
        &tx,
        3,
        false,
        &["Move incomplete (1 skipped); source kept at /src".to_string()],
        1,
    );
    let TaskStatus::Failed(msg) = next_status(&mut rx).await else {
        panic!("a failed transfer must report Failed");
    };
    assert!(
        msg.contains("Move incomplete"),
        "the reason, not a bare count, must reach the task: {msg}"
    );

    crate::handlers::popup_copy_move::report_transfer_status(
        &tx,
        4,
        false,
        &["boom".to_string()],
        2,
    );
    let TaskStatus::Failed(msg) = next_status(&mut rx).await else {
        panic!("a partial transfer must report Failed");
    };
    assert_eq!(msg, "Completed with errors: 1 failed");
}
