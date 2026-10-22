use super::*;

#[tokio::test]
async fn test_7z_create_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_create_dir.7z");
    create_empty_7z(&archive_path);

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    archive_fs.create_dir(Path::new("new_dir")).await.unwrap();
    let entry = archive_fs.get_entry(Path::new("new_dir")).unwrap();
    assert!(entry.is_dir);
    assert_eq!(entry.name, "new_dir");

    archive_fs.create_dir(Path::new("a/b/c")).await.unwrap();
    let nested = archive_fs.get_entry(Path::new("a/b/c")).unwrap();
    assert!(nested.is_dir);
}

#[tokio::test]
async fn test_local_fs_create_7z() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("new_archive.7z");

    let local_fs = LocalFs::new();
    local_fs.create_file(&path).await.unwrap();

    // Verify it's a valid, empty 7z
    let reader = ArchiveReader::open(&path, Password::empty()).expect("open archive ok");
    assert_eq!(reader.archive().files.len(), 0);
}

#[tokio::test]
async fn test_7z_read_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_read.7z");
    create_7z_with_file(&archive_path, "hello.txt", b"Hello, 7z!");

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let data = archive_fs.read_file(Path::new("hello.txt")).await.unwrap();
    assert_eq!(data, b"Hello, 7z!");
}

#[tokio::test]
async fn test_7z_write_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_write.7z");
    create_empty_7z(&archive_path);

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    archive_fs
        .write_file(Path::new("new.txt"), b"written content")
        .await
        .unwrap();

    let data = archive_fs.read_file(Path::new("new.txt")).await.unwrap();
    assert_eq!(data, b"written content");
}

#[tokio::test]
async fn test_7z_delete_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_delete.7z");

    {
        let mut writer = ArchiveWriter::create(&archive_path).unwrap();
        let e1 = sevenz_rust2::ArchiveEntry::new_file("keep.txt");
        writer
            .push_archive_entry(e1, Some(std::io::Cursor::new(b"keep")))
            .unwrap();
        let e2 = sevenz_rust2::ArchiveEntry::new_file("remove.txt");
        writer
            .push_archive_entry(e2, Some(std::io::Cursor::new(b"remove")))
            .unwrap();
        writer.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    archive_fs
        .delete(Path::new("remove.txt"), false)
        .await
        .unwrap();

    assert!(archive_fs.get_entry(Path::new("remove.txt")).is_none());
    let data = archive_fs.read_file(Path::new("keep.txt")).await.unwrap();
    assert_eq!(data, b"keep");
}

#[tokio::test]
async fn test_7z_rename_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_rename.7z");
    create_7z_with_file(&archive_path, "original.txt", b"rename me");

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    archive_fs
        .rename(Path::new("original.txt"), Path::new("renamed.txt"))
        .await
        .unwrap();

    assert!(archive_fs.get_entry(Path::new("original.txt")).is_none());
    let data = archive_fs
        .read_file(Path::new("renamed.txt"))
        .await
        .unwrap();
    assert_eq!(data, b"rename me");
}

#[tokio::test]
async fn test_7z_set_modified_time() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_mtime.7z");
    create_7z_with_file(&archive_path, "file.txt", b"content");

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let old_time = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    let ok = archive_fs
        .set_modified_time(Path::new("file.txt"), old_time)
        .await;
    assert!(ok, "set_modified_time should succeed");

    let entry = archive_fs.get_entry(Path::new("file.txt")).unwrap();
    let modified = entry.modified.unwrap();

    let diff = if modified > old_time {
        modified.duration_since(old_time).unwrap()
    } else {
        old_time.duration_since(modified).unwrap()
    };
    assert!(
        diff.as_secs() <= 2,
        "Timestamp not preserved. Expected ~{old_time:?}, got {modified:?}"
    );
}

#[tokio::test]
async fn test_7z_rename_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_rename_dir.7z");

    {
        let mut writer = ArchiveWriter::create(&archive_path).unwrap();
        let e1 = sevenz_rust2::ArchiveEntry::new_directory("old_dir");
        writer
            .push_archive_entry(e1, None::<std::io::Cursor<Vec<u8>>>)
            .unwrap();
        let e2 = sevenz_rust2::ArchiveEntry::new_file("old_dir/inner.txt");
        writer
            .push_archive_entry(e2, Some(std::io::Cursor::new(b"inner")))
            .unwrap();
        writer.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    archive_fs
        .rename(Path::new("old_dir"), Path::new("new_dir"))
        .await
        .unwrap();

    assert!(archive_fs.get_entry(Path::new("old_dir")).is_none());
    assert!(
        archive_fs
            .get_entry(Path::new("old_dir/inner.txt"))
            .is_none()
    );
    assert!(archive_fs.get_entry(Path::new("new_dir")).is_some());
    assert!(
        archive_fs
            .get_entry(Path::new("new_dir/inner.txt"))
            .is_some()
    );
    assert_eq!(
        archive_fs
            .read_file(Path::new("new_dir/inner.txt"))
            .await
            .unwrap(),
        b"inner"
    );
}
