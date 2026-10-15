use super::*;
use tokio::sync::mpsc;

// Test new() sets up debouncer and empty watched_paths
#[test]
fn test_new_initializes_state() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let watcher = AppWatcher::new(&tx);
    assert!(watcher.is_ok());
    let watcher = watcher.unwrap();
    assert_eq!(watcher.watched_paths.len(), 0);
}

// Test watch() adds a new path
#[test]
fn test_watch_adds_new_path() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut watcher = AppWatcher::new(&tx).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let res = watcher.watch(&path);
    assert!(res.is_ok());
    assert!(watcher.watched_paths.contains(&path));
}

#[test]
fn test_unwatch_removes_path() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut watcher = AppWatcher::new(&tx).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    watcher.watch(&path).unwrap();
    assert!(watcher.watched_paths.contains(&path));
    watcher.unwatch(&path).unwrap();
    assert!(!watcher.watched_paths.contains(&path));
}

#[test]
fn test_update_watched_paths_sync() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut watcher = AppWatcher::new(&tx).unwrap();
    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();
    let p1 = dir1.path().to_path_buf();
    let p2 = dir2.path().to_path_buf();
    watcher.watch(&p1).unwrap();
    watcher
        .update_watched_paths(std::slice::from_ref(&p2))
        .unwrap();
    assert!(!watcher.watched_paths.contains(&p1));
    assert!(watcher.watched_paths.contains(&p2));
}

/// This test ensures that the debouncer is indeed using a `FileIdMap`.
/// If someone changes the type to `NoCache`, this test will fail to compile.
#[test]
fn test_debouncer_uses_file_id_map() {
    // This is a type-level assertion.
    // We try to pass the debouncer to a function that explicitly expects FileIdMap.
    fn assert_file_id_map_cache<W: notify::Watcher>(_d: &Debouncer<W, FileIdMap>) {}

    let (tx, _rx) = mpsc::unbounded_channel();
    let watcher = AppWatcher::new(&tx).unwrap();

    assert_file_id_map_cache(&watcher.debouncer);
}
