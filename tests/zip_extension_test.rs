use fm::fs::fs_archive::ArchiveFs;
use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::FileSystemProvider;
use std::fs::File;
use std::path::Path;

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
    archive_fs.create_dir(Path::new("new_dir")).unwrap();

    // 4. Verify directory exists in archive
    let entry = archive_fs.get_entry(Path::new("new_dir")).unwrap();
    assert!(entry.file_entry.is_dir);
    assert_eq!(entry.file_entry.name, "new_dir");

    // 5. Create a nested directory
    archive_fs.create_dir(Path::new("a/b/c")).unwrap();
    let entry_nested = archive_fs.get_entry(Path::new("a/b/c")).unwrap();
    assert!(entry_nested.file_entry.is_dir);
}

#[tokio::test]
async fn test_local_fs_create_zip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let zip_path = temp_dir.path().join("new_archive.zip");

    let local_fs = LocalFs::new();
    local_fs.create_file(&zip_path).unwrap();

    // Verify it's a valid zip
    let file = File::open(&zip_path).unwrap();
    let archive = zip::ZipArchive::new(file).unwrap();
    assert_eq!(archive.len(), 0);
}
