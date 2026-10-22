use super::*;

#[tokio::test]
async fn test_zip_create_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_create_dir.zip");

    // 1. Create an initial empty zip
    {
        let file = File::create(&archive_path).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    // 3. Create a directory
    archive_fs.create_dir(Path::new("new_dir")).await.unwrap();

    // 4. Verify directory exists in archive
    let entry = archive_fs.get_entry(Path::new("new_dir")).unwrap();
    assert!(entry.is_dir);
    assert_eq!(entry.name, "new_dir");

    // 5. Create a nested directory
    archive_fs.create_dir(Path::new("a/b/c")).await.unwrap();
    let entry_nested = archive_fs.get_entry(Path::new("a/b/c")).unwrap();
    assert!(entry_nested.is_dir);
}

#[tokio::test]
async fn test_local_fs_create_zip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let zip_path = temp_dir.path().join("new_archive.zip");

    let local_fs = LocalFs::new();
    local_fs.create_file(&zip_path).await.unwrap();

    // Verify it's a valid zip
    let file = File::open(&zip_path).unwrap();
    let archive = zip::ZipArchive::new(file).unwrap();
    assert_eq!(archive.len(), 0);
}

#[tokio::test]
async fn test_zip_dot_dot_resolves_to_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_dotdot.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.add_directory("folder", options).unwrap();
        zip.start_file("folder/file.txt", options).unwrap();
        zip.write_all(b"hello").unwrap();
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    // "folder/.." is the parent (root), so it must be treated as a directory.
    assert!(archive_fs.is_dir(Path::new("folder")).await);
    assert!(archive_fs.is_dir(Path::new("folder/..")).await);
    assert!(archive_fs.is_dir(Path::new("/folder/..")).await);
    assert!(archive_fs.exists(Path::new("folder/..")).await);
    assert!(archive_fs.is_dir(Path::new("..")).await);

    // Nested resolution: "a/b/.." == "a"
    assert!(archive_fs.is_dir(Path::new("folder/../folder")).await);
    assert!(!archive_fs.is_dir(Path::new("folder/../file.txt")).await);
    assert!(!archive_fs.exists(Path::new("folder/../file.txt")).await);
}
