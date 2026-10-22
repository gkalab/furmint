use super::*;

#[tokio::test]
async fn test_mock_simple_move_conflict_overwrite() {
    let fs = MockFileSystem::default();
    let src = PathBuf::from("/src.txt");
    let dst = PathBuf::from("/dst.txt");
    fs.files
        .lock()
        .await
        .insert(src.clone(), FakeEntry { is_dir: false });
    fs.files
        .lock()
        .await
        .insert(dst.clone(), FakeEntry { is_dir: false }); // Simulate existing dest

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (decision_tx, decision_rx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(decision_rx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &fs,
            dest_fs: &fs,
            action: CopyMoveAction::Copy,
            cancel: &Arc::new(AtomicBool::new(false)),
            tx: &tx,
            id: 1,
            total: 1,
            total_bytes,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src,
        dest: &dst,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };
    // Spawn task to send overwrite decision
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict {
                task_id: _id,
                path: _path,
                conflict_type: _ty,
            }) = event
            {
                let _ = decision_tx.send(TaskDecision::Overwrite).await;
            }
        }
    });
    // Call operation
    let result = recursive_op(ctx, &mut decision_state).await;
    assert!(result.is_ok());
    // dst must exist
    assert!(fs.files.lock().await.contains_key(&dst));
    // src remains for Copy
    assert!(fs.files.lock().await.contains_key(&src));
}

#[tokio::test]
async fn test_move_rename_optimization_no_conflict() {
    let fs = MockFileSystem::default();
    let src = PathBuf::from("/src.txt");
    let dst = PathBuf::from("/dst.txt");
    fs.files
        .lock()
        .await
        .insert(src.clone(), FakeEntry { is_dir: false });

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &fs,
            dest_fs: &fs,
            action: CopyMoveAction::Move,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 2,
            total_bytes,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src,
        dest: &dst,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // If it triggers a conflict, it will wait for decision and we didn't send one,
    // but we can check the events.
    let res = recursive_op(ctx, &mut decision_state);

    // We wrap it in timeout to avoid hanging if the bug is present
    let res = tokio::time::timeout(std::time::Duration::from_millis(500), res).await;

    if let Ok(r) = res {
        assert!(r.is_ok(), "Operation failed: {:?}", r.err());
    } else {
        // Check if there was a conflict event
        if let Ok(UiEvent::Alert(AlertEvent::Conflict { .. })) = rx.try_recv() {
            panic!("BUG DETECTED: Conflict triggered for successful move!");
        }
        panic!("Operation timed out - likely waiting for conflict resolution!");
    }

    assert!(!fs.exists(&src).await);
    assert!(fs.exists(&dst).await);
}

#[tokio::test]
async fn test_move_rename_optimization_directory() {
    let fs = MockFileSystem::default();
    let src_dir = PathBuf::from("/src_dir");
    let dest_dir = PathBuf::from("/dest_dir");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_dir.clone(), FakeEntry { is_dir: true });
        files.insert(src_dir.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(src_dir.join("file2.txt"), FakeEntry { is_dir: false });
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &fs,
            dest_fs: &fs,
            action: CopyMoveAction::Move,
            cancel: &Arc::new(AtomicBool::new(false)),
            tx: &tx,
            id: 1,
            total: 3, // src_dir + file1.txt + file2.txt
            total_bytes,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_dir,
        dest: &dest_dir,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());

    // Verify the progress update the UI applies. Progress events are coalesced
    // keep-latest on the bus, so the snapshot notification carries the newest
    // value — resolve it the way `dispatch_ui_event` does.
    let mut progress_events = 0;
    let mut last_p = 0;
    while let Ok(event) = rx.try_recv() {
        let event = match event {
            UiEvent::TaskSnapshot { task_id, field } => tx
                .take_coalesced(task_id, field)
                .unwrap_or_else(|| panic!("snapshot for task {task_id} had no pending value")),
            UiEvent::Task(e) => e,
            _ => continue,
        };
        if let TaskEvent::UpdateProgress { processed: p, .. } = event {
            progress_events += 1;
            last_p = p;
        }
    }

    // The rename moved the whole subtree, so all 3 items (dir + 2 files)
    // are credited at once and the bar reaches total.
    assert_eq!(last_p, 3);
    assert!(progress_events >= 1);

    assert!(!fs.exists(&src_dir).await);
    assert!(fs.exists(&dest_dir).await);
    assert!(fs.exists(&dest_dir.join("file1.txt")).await);
    assert!(fs.exists(&dest_dir.join("file2.txt")).await);
}

/// A whole-source move must ask `rename` whether the two paths share a
/// filesystem, rather than trusting a context key: the kernel is the authority,
/// and bind mounts, overlay layers and subvolumes make any key or device-id
/// guess unreliable.
///
/// The two providers deliberately share a context key, so the optimization
/// attempts the rename; the mock refuses it with `EXDEV`. The move must then
/// degrade to copy-then-delete rather than failing or leaving the source
/// behind.
#[tokio::test]
async fn cross_device_move_falls_back_to_copy() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }
    *fs.rename_crosses_devices.lock().await = true;

    let (raw_tx, _rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();

    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &fs,
            dest_fs: &fs,
            action: CopyMoveAction::Move,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 2,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };

    recursive_op(ctx, &mut DecisionState::new()).await.unwrap();

    assert_eq!(
        *fs.rename_attempts.lock().await,
        1,
        "the move must consult the kernel before falling back to a copy"
    );
    let copies = fs.copies.lock().await;
    assert!(
        copies.contains(&(src_root.join("file1.txt"), dest_root.join("file1.txt"))),
        "a refused rename must fall back to copying the tree: {copies:?}"
    );
    drop(copies);
    assert!(
        !fs.exists(&src_root).await,
        "the fallback copy must still complete the move and remove the source"
    );
    assert!(fs.exists(&dest_root.join("file1.txt")).await);
}
