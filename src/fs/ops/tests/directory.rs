use super::*;

#[tokio::test]
async fn test_handle_directory_dest_exists_as_dir_merge() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.clone(), FakeEntry { is_dir: true });
        files.insert(dest_root.join("existing.txt"), FakeEntry { is_dir: false });
    }

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
            action: CopyMoveAction::Copy,
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

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(fs.exists(&dest_root.join("file1.txt")).await);
    assert!(fs.exists(&dest_root.join("existing.txt")).await);
}

#[tokio::test]
async fn test_handle_directory_download_success() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    *fs.download_result.lock().await = Some(Ok(()));

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
            action: CopyMoveAction::Copy,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 1,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
    assert!(fs.exists(&dest_root).await);
}

#[tokio::test]
async fn test_handle_directory_download_failure() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    *fs.download_result.lock().await = Some(Err(anyhow::anyhow!("Download failed")));

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
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
            total: 1,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::TaskError { .. }) = event {
                let _ = dtx.send(TaskDecision::Skip).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_directory_read_dir_error() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    *fs.fail_next_read_dir.lock().await = true;

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
            action: CopyMoveAction::Copy,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 1,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_directory_create_dir_error() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    *fs.fail_next_create_dir.lock().await = 1;

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
            action: CopyMoveAction::Copy,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 1,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_directory_dest_exists_as_file_skip() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.clone(), FakeEntry { is_dir: false });
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
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
            total: 1,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::Skip).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
}

#[tokio::test]
async fn test_handle_directory_dest_exists_as_file_cancel() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
        files.insert(dest_root.clone(), FakeEntry { is_dir: false });
    }

    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let (dtx, drx_real) = mpsc::channel(1);
    let processed = Arc::new(AtomicUsize::new(0));
    let decision_rx = Arc::new(Mutex::new(drx_real));
    let cancel = Arc::new(AtomicBool::new(false));
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
            total: 1,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };

    let mut decision_state = DecisionState::new();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let UiEvent::Alert(AlertEvent::Conflict { .. }) = event {
                let _ = dtx.send(TaskDecision::Cancel).await;
            }
        }
    });

    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok());
}

#[tokio::test]
async fn test_handle_directory_download_fallback() {
    let fs = MockFileSystem::default();
    let src_root = PathBuf::from("/src");
    let dest_root = PathBuf::from("/dest");

    {
        let mut files = fs.files.lock().await;
        files.insert(src_root.clone(), FakeEntry { is_dir: true });
        files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
    }

    // copy_to_local returns None, triggering fallback to recursive logic
    *fs.download_result.lock().await = None;

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
            action: CopyMoveAction::Copy,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 1,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src: &src_root,
        dest: &dest_root,
    };

    let mut decision_state = DecisionState::new();
    let res = recursive_op(ctx, &mut decision_state).await;

    assert!(res.is_ok(), "Fallback logic failed: {:?}", res.err());
    assert!(fs.exists(&dest_root).await, "Dest root was not created");
    assert!(
        fs.exists(&dest_root.join("file1.txt")).await,
        "Child file was not copied"
    );
}
