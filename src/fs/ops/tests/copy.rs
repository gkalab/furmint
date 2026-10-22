use super::*;

#[tokio::test]
async fn test_recursive_copy_nested() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    // Hierarchy:
    // /src/file1.txt
    // /src/subdir/file2.txt

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("subdir"), FakeEntry { is_dir: true });
        files.insert(
            src_root.join("subdir").join("file2.txt"),
            FakeEntry { is_dir: false },
        );
    }

    let (raw_tx, _rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (_dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
    let total_bytes = 0; // Mock doesn't track size
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &fs,
            dest_fs: &fs,
            action: CopyMoveAction::Copy,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 3,
            total_bytes,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok(), "Recursive copy failed: {:?}", res.err());

    // Verify destination structure
    assert!(fs.exists(&dest_root).await);
    assert!(fs.exists(&dest_root.join("file1.txt")).await);
    assert!(fs.exists(&dest_root.join("subdir")).await);
    assert!(fs.exists(&dest_root.join("subdir").join("file2.txt")).await);
}

#[tokio::test]
async fn test_recursive_copy_overwrite_all() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

        // f1 and f2 exist in dest
        files.insert(dest_root.clone(), FakeEntry { is_dir: true });
        files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
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
            total: 2,
            total_bytes,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task: send OverwriteAll on first conflict
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::OverwriteAll).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(decision_state.overwrite_all);

    let copies = fs.copies.lock().await;
    assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f1.txt")));
    assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f2.txt")));
}

#[tokio::test]
async fn test_recursive_copy_cancel() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

        // Both exist in dest to ensure conflict
        files.insert(dest_root.clone(), FakeEntry { is_dir: true });
        files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
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
            total: 2,
            total_bytes,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task: send Cancel on first conflict
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::Cancel).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());

    // At most one file could have been processed (the one that triggered the conflict)
    // But since it returned on Cancel, no copy should have been performed for that file either.
    let copies = fs.copies.lock().await;
    assert_eq!(copies.len(), 0, "No files should have been copied");
}

#[tokio::test]
async fn test_recursive_copy_skip_all() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

        // f1 exists in dest
        files.insert(dest_root.clone(), FakeEntry { is_dir: true });
        files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
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
            total: 2,
            total_bytes,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Decision task
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::SkipAll).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(decision_state.skip_all);

    let copies = fs.copies.lock().await;
    assert!(!copies.iter().any(|(_, d)| d == &dest_root.join("f1.txt")));
    assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f2.txt")));
}

#[tokio::test]
async fn test_recursive_op_retry() {
    let fs = MockFileSystem::default();
    let src = PathBuf::from("/src.txt");
    let dst = PathBuf::from("/dst.txt");
    fs.files
        .lock()
        .await
        .insert(src.clone(), FakeEntry { is_dir: false });

    // First copy attempt fails, second succeeds
    {
        let mut fails = fs.fail_next_copy.lock().await;
        *fails = 1;
    }

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

    // Spawn task to send retry decision
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = decision_tx.send(TaskDecision::Retry).await;
            }
        }
    });

    let result = recursive_op(ctx, &mut decision_state).await;

    assert!(result.is_ok());
    assert!(fs.files.lock().await.contains_key(&dst));
}

#[tokio::test]
async fn test_recursive_op_error_skip_all() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    // Both copies will fail
    {
        let mut fails = fs.fail_next_copy.lock().await;
        *fails = 2;
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &fs,
            dest_fs: &fs,
            action: CopyMoveAction::Copy,
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
        src: &src_root,
        dest: &dest_root,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Send SkipAll on the first error
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = dtx.send(TaskDecision::SkipAll).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(decision_state.skip_all, "skip_all should be set");

    // No copies should have succeeded (both failed)
    let copies = fs.copies.lock().await;
    assert_eq!(copies.len(), 0, "No files should have been copied");
}

#[tokio::test]
async fn test_recursive_op_error_cancel() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    // Both copies will fail
    {
        let mut fails = fs.fail_next_copy.lock().await;
        *fails = 2;
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &fs,
            dest_fs: &fs,
            action: CopyMoveAction::Copy,
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
        src: &src_root,
        dest: &dest_root,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Send Cancel on the first error
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = dtx.send(TaskDecision::Cancel).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(
        cancel.load(std::sync::atomic::Ordering::Relaxed),
        "cancel flag should be set"
    );

    // No copies should have succeeded (first errored, second cancelled)
    let copies = fs.copies.lock().await;
    assert_eq!(copies.len(), 0, "No files should have been copied");
}

#[tokio::test]
async fn test_recursive_op_error_skip() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
        files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });
    }

    // Both copies will fail
    {
        let mut fails = fs.fail_next_copy.lock().await;
        *fails = 2;
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let total_bytes = 0;
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();
    let ctx = RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs: &fs,
            dest_fs: &fs,
            action: CopyMoveAction::Copy,
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
        src: &src_root,
        dest: &dest_root,
    };
    let mut decision_state = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: std::time::Instant::now(),
    };

    // Send Skip on each error
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = dtx.send(TaskDecision::Skip).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(!decision_state.skip_all, "skip_all should NOT be set");
    assert!(
        !cancel.load(std::sync::atomic::Ordering::Relaxed),
        "cancel flag should NOT be set"
    );

    // No copies should have succeeded (both failed and were skipped individually)
    let copies = fs.copies.lock().await;
    assert_eq!(copies.len(), 0, "No files should have been copied");
}
