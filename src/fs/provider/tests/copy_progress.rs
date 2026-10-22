use super::*;

#[tokio::test]
async fn test_copy_with_progress_small_file_whole() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("small.bin");
    let dst = dir.path().join("small_copy.bin");
    // Below the local whole-file limit => single whole-file copy, one progress event
    let data: Vec<u8> = (0..1024).map(|i| (i % 251).try_into().unwrap()).collect();
    std::fs::write(&src, &data).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    fs().copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();

    let progress = byte_events(&bus, drain(&mut rx));
    assert_eq!(
        progress,
        vec![(data.len() as u64, data.len() as u64)],
        "small file should emit a single progress event"
    );
    assert_eq!(std::fs::read(&dst).unwrap(), data);
    assert!(
        leftover_temps(dir.path(), "small_copy.bin").is_empty(),
        "no temp file may remain after a successful copy"
    );
}

#[tokio::test]
async fn test_copy_with_progress_multi_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("big_copy.bin");
    // 33MB file (> the local whole-file limit) => 16MB chunks => multiple writes
    let len = 33 * 1024 * 1024;
    let data: Vec<u8> = (0..len).map(|i| (i % 251).try_into().unwrap()).collect();
    std::fs::write(&src, &data).unwrap();

    let flaky = flaky_copy_fs(usize::MAX, usize::MAX);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();

    assert!(
        flaky.writes.load(std::sync::atomic::Ordering::Relaxed) >= 2,
        "a 33MB file must take the chunked path, not a single whole-file write"
    );

    // The per-chunk updates are coalesced keep-latest, so the burst the UI
    // sees resolves to exactly one notification carrying the final value.
    let events = drain(&mut rx);
    assert!(
        events
            .iter()
            .all(|e| matches!(e, crate::tasks::UiEvent::TaskSnapshot { .. })),
        "expected only coalesced notifications, got {events:?}"
    );
    let progress = byte_events(&bus, events);
    assert_eq!(
        progress,
        vec![(data.len() as u64, data.len() as u64)],
        "the coalesced progress must be the newest value, not a stale one"
    );
    assert_eq!(std::fs::read(&dst).unwrap(), data);
    assert!(
        leftover_temps(dir.path(), "big_copy.bin").is_empty(),
        "no temp file may remain after a successful copy"
    );
}

#[tokio::test]
async fn test_copy_with_progress_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("empty.bin");
    let dst = dir.path().join("empty_copy.bin");
    std::fs::write(&src, b"").unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    fs().copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();
    assert!(dst.exists());
    assert_eq!(std::fs::read(&dst).unwrap(), Vec::<u8>::new());
}

#[tokio::test]
async fn test_copy_with_progress_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("cancel.bin");
    let dst = dir.path().join("cancel_copy.bin");
    std::fs::write(&src, vec![0u8; 1024]).unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(true));
    fs().copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();
    assert!(!dst.exists());
    assert!(
        leftover_temps(dir.path(), "cancel_copy.bin").is_empty(),
        "a cancelled copy must remove the scratch file it reserved"
    );
}

#[tokio::test]
async fn test_copy_with_progress_cancel_mid_chunk_leaves_no_partial() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("big_copy.bin");
    // 33MB => 16MB chunks => the copy is cancelled after chunk 2 of 3.
    std::fs::write(&src, vec![0u8; 33 * 1024 * 1024]).unwrap();

    let flaky = flaky_copy_fs(2, usize::MAX);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &flaky.cancel, 0))
        .await
        .unwrap();

    assert!(
        !dst.exists(),
        "cancelled copy must not commit the destination"
    );
    assert!(
        leftover_temps(dir.path(), "big_copy.bin").is_empty(),
        "cancelled copy must remove its temp file"
    );
}

#[tokio::test]
async fn test_copy_with_progress_write_failure_leaves_no_partial() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("big_copy.bin");
    // 33MB => 16MB chunks; the second chunk write fails.
    std::fs::write(&src, vec![0u8; 33 * 1024 * 1024]).unwrap();

    let flaky = flaky_copy_fs(0, 1);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let res = flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &flaky.cancel, 0))
        .await;
    assert!(res.is_err(), "the injected write failure must surface");

    assert!(!dst.exists(), "failed copy must not commit the destination");
    assert!(
        leftover_temps(dir.path(), "big_copy.bin").is_empty(),
        "failed copy must remove its temp file"
    );
}

/// A successful copy must sync the temporary file (the commit point) exactly
/// once before renaming it, then best-effort sync the destination directory.
#[tokio::test]
async fn test_copy_with_progress_syncs_before_rename_and_dir_after() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("data.bin");
    let dst = dir.path().join("data_copy.bin");
    std::fs::write(&src, b"durable").unwrap();

    let flaky = flaky_copy_fs(usize::MAX, usize::MAX);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();

    assert_eq!(
        flaky.syncs.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "the temp file must be synced once before the rename"
    );
    assert_eq!(
        flaky
            .parent_syncs
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "the destination directory must be synced after the rename"
    );
    assert_eq!(std::fs::read(&dst).unwrap(), b"durable");
    assert!(
        leftover_temps(dir.path(), "data_copy.bin").is_empty(),
        "a completed copy must not leave a scratch file behind"
    );
}

/// The sync is the commit point: when it fails the rename must not happen, so
/// no truncated file is published and the temp file is cleaned up.
#[tokio::test]
async fn test_copy_with_progress_sync_failure_does_not_commit() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("data.bin");
    let dst = dir.path().join("data_copy.bin");
    std::fs::write(&src, b"data").unwrap();

    let mut flaky = flaky_copy_fs(usize::MAX, usize::MAX);
    flaky.fail_sync = true;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    let res = flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await;

    assert!(res.is_err(), "a failed sync must surface as a copy error");
    assert!(
        !dst.exists(),
        "a failed sync must not rename the temp file into place"
    );
    assert!(
        leftover_temps(dir.path(), "data_copy.bin").is_empty(),
        "a failed sync must remove its temp file"
    );
}
