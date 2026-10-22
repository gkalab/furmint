#[cfg(unix)]
use super::*;

/// A child that cannot be removed is reported once, by name; its parent is not
/// also reported as "not empty", and the rest of the batch still runs.
#[cfg(unix)]
#[tokio::test]
async fn test_delete_collects_per_entry_failures_without_double_reporting() {
    use std::os::unix::fs::PermissionsExt;

    let fs = LocalFs::new();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    let locked = root.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    // A file inside a directory we cannot write to: unlinking it fails.
    std::fs::write(locked.join("stuck.txt"), b"s").unwrap();
    // A sibling that can be removed, so the batch must not abort on the first
    // failure.
    std::fs::write(root.join("removable.txt"), b"r").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

    let err = fs
        .delete(&root, true)
        .await
        .expect_err("a stuck child must be reported");

    let msg = err.to_string();
    assert!(msg.contains("stuck.txt"), "the child is named: {msg}");
    assert!(
        !msg.contains("removable.txt"),
        "entries that could be removed are not reported: {msg}"
    );
    assert!(
        !root.join("removable.txt").exists(),
        "removing a sibling must not be skipped because another child failed"
    );
    assert!(
        root.join("locked").exists(),
        "a directory with a stuck child is left in place"
    );

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_dir_all(&root).ok();
}

/// The write probe accepts a normal directory and refuses one without write
/// permission, and leaves nothing behind either way.
#[cfg(unix)]
#[tokio::test]
async fn test_check_writable_refuses_a_read_only_directory() {
    use std::os::unix::fs::PermissionsExt;

    let fs = LocalFs::new();
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("ro");
    std::fs::create_dir(&dir).unwrap();

    fs.check_writable(&dir)
        .await
        .expect("a normal directory is writable");

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let err = fs
        .check_writable(&dir)
        .await
        .expect_err("a mode-555 directory must be refused");
    assert!(err.to_string().contains("not writable"), "{err}");

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "the probe must clean up after itself"
    );
}

/// Local paths expose an inode identity so the walk can detect cycles.
#[cfg(unix)]
#[tokio::test]
async fn test_file_id_is_stable_for_aliases() {
    let fs = LocalFs::new();
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("real");
    std::fs::create_dir(&dir).unwrap();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&dir, &link).unwrap();

    let dir_id = fs.file_id(&dir).await.expect("a real dir has an id");
    let link_id = fs
        .file_id(&link)
        .await
        .expect("the link resolves to the same id");
    assert_eq!(dir_id, link_id);
}
