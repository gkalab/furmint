use super::*;

/// Each backend family answers with one profile instead of call sites probing
/// its type: both local and remote support every operation, an archive rename
/// is a container rewrite, and writability can be lowered without changing the
/// rest.
#[test]
fn test_capabilities_describe_each_backend_family() {
    let local = Capabilities::for_kind(BackendKind::Local);
    assert!(local.is_local() && !local.is_archive());
    assert!(local.writable && local.seekable && local.random_access);
    assert!(local.has_directories && local.atomic_rename && local.links);
    assert_eq!(local.latency, Latency::Local);

    let archive = Capabilities::for_kind(BackendKind::Archive);
    assert!(archive.is_archive() && !archive.is_local());
    assert!(archive.seekable && archive.random_access && archive.has_directories);
    assert!(
        !archive.atomic_rename,
        "a rename rewrites the archive container rather than moving an entry"
    );
    assert_eq!(archive.latency, Latency::Local);

    let remote = Capabilities::for_kind(BackendKind::Remote);
    assert!(!remote.is_local() && !remote.is_archive());
    assert!(remote.atomic_rename && remote.links);
    assert_eq!(remote.latency, Latency::Network);

    let read_only = archive.with_writable(false);
    assert!(!read_only.writable);
    assert_eq!(read_only.kind, BackendKind::Archive);
    assert!(!read_only.atomic_rename, "the rest of the profile is kept");
}

/// `LocalFs` reports the local profile, and `is_writable` is read from it.
#[test]
fn test_local_provider_reports_local_capabilities() {
    let fs = LocalFs::new();
    let caps = fs.capabilities();
    assert_eq!(caps.kind, BackendKind::Local);
    assert_eq!(caps, Capabilities::for_kind(BackendKind::Local));
    assert!(fs.is_writable());
}

/// The latency class, not a hard-coded constant, decides how a copy moves its
/// bytes: local files stay whole-file for longer and stream in larger chunks
/// than a network transfer.
#[test]
fn test_latency_sets_transfer_chunking() {
    assert!(
        Latency::Local.whole_file_limit() > Latency::Network.whole_file_limit(),
        "a local copy can afford a larger whole-file transfer"
    );
    assert_eq!(
        Latency::Local.chunk_size(1),
        16 * 1024 * 1024,
        "a local stream uses a large fixed buffer"
    );
    assert!(
        Latency::Network.chunk_size(1) < Latency::Network.chunk_size(200 * 1024 * 1024),
        "a network stream scales its chunk with the file"
    );
}

/// A 6 MB file is below the local whole-file limit but above the network one:
/// the same copy takes one write locally and several over the network.
#[tokio::test]
async fn test_copy_chunking_consults_the_backend_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("medium.bin");
    // 6MB: local whole-file limit is 32MB, network is 4MB.
    std::fs::write(&src, vec![0u8; 6 * 1024 * 1024]).unwrap();

    let local = flaky_copy_fs(usize::MAX, usize::MAX);
    let local_dst = dir.path().join("local.bin");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    local
        .copy_with_progress(&src, &local_dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();
    assert_eq!(
        local.writes.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a local copy below its whole-file limit must not take the chunked path"
    );

    let mut remote = flaky_copy_fs(usize::MAX, usize::MAX);
    remote.latency = Some(Latency::Network);
    let remote_dst = dir.path().join("remote.bin");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    remote
        .copy_with_progress(&src, &remote_dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();
    assert!(
        remote.writes.load(std::sync::atomic::Ordering::Relaxed) >= 2,
        "a network copy above its whole-file limit must be streamed"
    );
    assert_eq!(std::fs::read(&remote_dst).unwrap().len(), 6 * 1024 * 1024);
}
