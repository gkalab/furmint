use super::*;

#[tokio::test]
async fn test_local_fs_create_and_delete_dir() {
    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();
    let test_dir = temp_dir.join("fm_test_local_fs_dir");

    // Clean up if exists
    if test_dir.exists() {
        std::fs::remove_dir_all(&test_dir).ok();
    }

    // Create
    assert!(fs.create_dir(&test_dir).await.is_ok());
    assert!(test_dir.exists());

    // Delete
    assert!(fs.delete(&test_dir, false).await.is_ok());
    assert!(!test_dir.exists());
}

/// A recursive delete removes a whole tree, children before their parent.
#[tokio::test]
async fn test_delete_removes_a_whole_tree() {
    let fs = LocalFs::new();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    std::fs::create_dir_all(root.join("a/b")).unwrap();
    std::fs::write(root.join("top.txt"), b"t").unwrap();
    std::fs::write(root.join("a/b/deep.txt"), b"d").unwrap();

    fs.delete(&root, true).await.unwrap();

    assert!(!root.exists(), "a recursive delete removes the root");
}

/// A cancelled recursive delete is a no-op that reports success: the caller
/// reports the cancellation, and nothing is half-removed.
#[tokio::test]
async fn test_cancelled_delete_removes_nothing() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let fs = LocalFs::new();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    for i in 0..8 {
        std::fs::create_dir_all(root.join(format!("d{i}"))).unwrap();
        std::fs::write(root.join(format!("d{i}/f.txt")), b"x").unwrap();
    }

    let cancel = Arc::new(AtomicBool::new(true));
    fs.delete_cancellable(&root, true, &cancel).await.unwrap();

    assert!(root.exists(), "a cancelled delete must not remove the root");
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        8,
        "an already-cancelled delete removes nothing"
    );
}

#[tokio::test]
async fn test_check_writable_rejects_a_missing_directory() {
    let fs = LocalFs::new();
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist");
    assert!(
        fs.check_writable(&missing).await.is_err(),
        "a missing destination directory cannot accept a write"
    );
}
