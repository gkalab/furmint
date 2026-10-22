use super::*;

#[test]
fn test_temp_file_guard_removes_on_drop() {
    let path = std::env::temp_dir().join("fm_test_guard_drop.txt");
    std::fs::write(&path, b"x").unwrap();
    {
        let _guard = TempFileGuard::new(path.clone());
    }
    assert!(!path.exists());
}

#[test]
fn test_temp_file_guard_release_keeps_file() {
    let path = std::env::temp_dir().join("fm_test_guard_release.txt");
    std::fs::write(&path, b"x").unwrap();
    let released = TempFileGuard::new(path.clone()).release();
    assert_eq!(released, path);
    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn test_sweep_stale_temp_files() {
    let temp_dir = std::env::temp_dir();
    // pid u32::MAX is never a real pid; its file must be swept
    let dead_file = temp_dir.join(format!("fm_{}_dead.txt", u32::MAX));
    std::fs::write(&dead_file, b"x").unwrap();
    // Our own pid must be left alone
    let alive_file = temp_dir.join(format!("fm_{}_alive.txt", std::process::id()));
    std::fs::write(&alive_file, b"x").unwrap();
    // Unrelated files must be left alone
    let other_file = temp_dir.join("fm_test_sweep_not_ours.txt");
    std::fs::write(&other_file, b"x").unwrap();

    sweep_stale_temp_files();

    assert!(!dead_file.exists());
    assert!(alive_file.exists());
    assert!(other_file.exists());
    let _ = std::fs::remove_file(&alive_file);
    let _ = std::fs::remove_file(&other_file);
}
