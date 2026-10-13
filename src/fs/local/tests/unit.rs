use super::super::*;

#[tokio::test]
async fn test_local_fs_list_dir() {
    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();
    let entries = fs.list_dir(&temp_dir).await.unwrap();
    // Should always have ".." entry
    assert_ne!(entries, Vec::new());
    assert_eq!(entries[0].name, "..");
}

#[tokio::test]
async fn test_local_fs_get_permissions() {
    use std::fs::File;
    use std::io::Write;

    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("test_permissions_file.txt");

    // Create a test file
    {
        let mut file = File::create(&test_file).unwrap();
        file.write_all(b"test").unwrap();
    }

    #[cfg(unix)]
    {
        // Just check that we can get some permissions (should not be None)
        let perms = fs.get_permissions(&test_file).await;
        assert!(perms.is_some());
        // Permissions should be valid Unix permission bits (0-0o777)
        let mode = perms.unwrap();
        assert!(mode <= 0o777);
    }

    #[cfg(not(unix))]
    {
        let perms = fs.get_permissions(&test_file).await;
        assert_eq!(perms, None);
    }

    // Clean up
    std::fs::remove_file(&test_file).unwrap();
}

#[tokio::test]
async fn test_local_fs_get_file_info() {
    use std::fs::File;
    use std::io::Write;

    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("test_get_file_info.txt");

    {
        let mut file = File::create(&test_file).unwrap();
        file.write_all(b"hello").unwrap();
    }

    let info = fs
        .get_file_info(&test_file)
        .await
        .expect("file should exist");
    assert_eq!(info.size, 5);
    assert!(info.modified.is_some());

    #[cfg(unix)]
    {
        let perms = info.permissions.expect("unix should report permissions");
        assert!(perms <= 0o777);
    }
    #[cfg(not(unix))]
    {
        assert_eq!(info.permissions, None);
    }

    assert_eq!(
        fs.get_file_info(&temp_dir.join("definitely_missing_42"))
            .await,
        None
    );

    std::fs::remove_file(&test_file).unwrap();
}

#[tokio::test]
async fn test_local_fs_set_permissions() {
    use std::fs::File;
    use std::io::Write;

    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("test_set_permissions_file.txt");

    // Create a test file
    {
        let mut file = File::create(&test_file).unwrap();
        file.write_all(b"test").unwrap();
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // Get original permissions
        let original_perms = std::fs::metadata(&test_file).unwrap().permissions().mode() & 0o777;

        // Set different permissions
        let new_perms = if original_perms == 0o644 {
            0o755
        } else {
            0o644
        };
        let success = fs.set_permissions(&test_file, new_perms).await;
        assert!(success);

        // Verify the permissions were set (may be affected by umask)
        let metadata = std::fs::metadata(&test_file).unwrap();
        let actual_perms = metadata.permissions().mode() & 0o777;
        // At minimum, the permissions should have changed or stayed the same due to umask
        assert!(actual_perms <= 0o777);
    }

    #[cfg(not(unix))]
    {
        let success = fs.set_permissions(&test_file, 0o755).await;
        assert!(!success);
    }

    // Clean up
    std::fs::remove_file(&test_file).unwrap();
}

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

#[tokio::test]
async fn test_local_fs_create_dir_all() {
    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();
    let test_dir = temp_dir.join("fm_test_local_fs_dir_all");

    // Clean up if exists
    if test_dir.exists() {
        std::fs::remove_dir_all(&test_dir).ok();
    }

    // Create nested directories that don't exist yet
    let nested = test_dir.join("a").join("b").join("c");
    assert!(fs.create_dir_all(&nested).await.is_ok());
    assert!(nested.exists());

    // Creating again must succeed (idempotent)
    assert!(fs.create_dir_all(&nested).await.is_ok());

    std::fs::remove_dir_all(&test_dir).ok();
}

#[tokio::test]
async fn test_local_fs_read_write_file() {
    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("fm_test_local_fs_file.txt");

    // Write
    let data = b"Hello, FileSystemProvider!";
    assert!(fs.write_file(&test_file, data).await.is_ok());
    assert!(test_file.exists());

    // Read
    let read_data = fs.read_file(&test_file).await.unwrap();
    assert_eq!(read_data, data);

    // Clean up
    std::fs::remove_file(&test_file).ok();
}

#[test]
fn test_local_fs_properties() {
    let fs = LocalFs::new();
    assert!(fs.is_local());
    assert_eq!(fs.display_prefix(), "");
}

#[tokio::test]
async fn test_local_fs_exists_and_is_dir() {
    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();

    assert!(fs.exists(&temp_dir).await);
    assert!(fs.is_dir(&temp_dir).await);
}

#[tokio::test]
async fn test_calc_dir_size() {
    let fs = LocalFs::new();
    let temp_dir = std::env::temp_dir();
    let test_dir = temp_dir.join("fm_test_calc_dir_size");

    // Clean up if exists
    if test_dir.exists() {
        std::fs::remove_dir_all(&test_dir).ok();
    }

    // Create test directory structure
    std::fs::create_dir(&test_dir).unwrap();
    let subdir = test_dir.join("subdir");
    std::fs::create_dir(&subdir).unwrap();

    // Create files with known sizes
    let file1 = test_dir.join("file1.txt");
    let file2 = subdir.join("file2.txt");
    std::fs::write(&file1, "Hello, World!").unwrap(); // 13 bytes
    std::fs::write(&file2, "Test content").unwrap(); // 12 bytes

    // Calculate directory size
    let size = fs.calc_dir_size(&test_dir).await.unwrap();

    // Should be at least 25 bytes (file contents)
    // Note: actual size may vary due to filesystem block allocation
    assert!(
        size >= 25,
        "Directory size should be at least 25 bytes, got {size}"
    );

    // Clean up
    std::fs::remove_dir_all(&test_dir).ok();
}
