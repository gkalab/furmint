use fm::fs::fs_archive::ArchiveFs;
use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::FileSystemProvider;
use fm::fs::provider::ProviderFileSystem;
use sevenz_rust2::{ArchiveReader, ArchiveWriter, Password};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

fn make_progress() -> fm::fs::traits::TaskProgressContext {
    fm::fs::traits::TaskProgressContext {
        id: 1,
        tx: tokio::sync::mpsc::unbounded_channel().0,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        processed_items: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }
}

fn create_empty_7z(path: &Path) {
    let writer = ArchiveWriter::create(path).expect("create writer ok");
    writer.finish().expect("finish ok");
}

fn create_7z_with_file(path: &Path, name: &str, content: &[u8]) {
    let mut writer = ArchiveWriter::create(path).expect("create writer ok");
    let entry = sevenz_rust2::ArchiveEntry::new_file(name);
    writer
        .push_archive_entry(entry, Some(std::io::Cursor::new(content)))
        .expect("push entry ok");
    writer.finish().expect("finish ok");
}

#[tokio::test]
async fn test_7z_create_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_create_dir.7z");
    create_empty_7z(&archive_path);

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    archive_fs.create_dir(Path::new("new_dir")).unwrap();
    let entry = archive_fs.get_entry(Path::new("new_dir")).unwrap();
    assert!(entry.file_entry.is_dir);
    assert_eq!(entry.file_entry.name, "new_dir");

    archive_fs.create_dir(Path::new("a/b/c")).unwrap();
    let nested = archive_fs.get_entry(Path::new("a/b/c")).unwrap();
    assert!(nested.file_entry.is_dir);
}

#[tokio::test]
async fn test_local_fs_create_7z() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("new_archive.7z");

    let local_fs = LocalFs::new();
    local_fs.create_file(&path).unwrap();

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
    let data = archive_fs.read_file(Path::new("hello.txt")).unwrap();
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
        .unwrap();

    let data = archive_fs.read_file(Path::new("new.txt")).unwrap();
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
    archive_fs.delete(Path::new("remove.txt"), false).unwrap();

    assert!(archive_fs.get_entry(Path::new("remove.txt")).is_none());
    let data = archive_fs.read_file(Path::new("keep.txt")).unwrap();
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
        .unwrap();

    assert!(archive_fs.get_entry(Path::new("original.txt")).is_none());
    let data = archive_fs.read_file(Path::new("renamed.txt")).unwrap();
    assert_eq!(data, b"rename me");
}

#[tokio::test]
async fn test_7z_set_modified_time() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_mtime.7z");
    create_7z_with_file(&archive_path, "file.txt", b"content");

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let old_time = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    let ok = archive_fs.set_modified_time(Path::new("file.txt"), old_time);
    assert!(ok, "set_modified_time should succeed");

    let entry = archive_fs.get_entry(Path::new("file.txt")).unwrap();
    let modified = entry.file_entry.modified.unwrap();

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
async fn test_7z_extract() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_extract.7z");

    {
        let mut writer = ArchiveWriter::create(&archive_path).unwrap();
        let entry = sevenz_rust2::ArchiveEntry::new_file("sub/hello.txt");
        writer
            .push_archive_entry(entry, Some(std::io::Cursor::new(b"extracted!")))
            .unwrap();
        writer.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let extract_dir = temp_dir.path().join("extracted");
    std::fs::create_dir(&extract_dir).unwrap();
    let dest_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));

    let progress = make_progress();

    // Extract the whole archive root into the directory
    let result = archive_fs
        .extract(Path::new("."), &dest_fs, &extract_dir, &progress)
        .await
        .unwrap();
    result.unwrap();

    let extracted = std::fs::read(extract_dir.join("sub/hello.txt")).unwrap();
    assert_eq!(extracted, b"extracted!");
}

#[tokio::test]
async fn test_7z_list_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_list.7z");

    {
        let mut writer = ArchiveWriter::create(&archive_path).unwrap();
        let e1 = sevenz_rust2::ArchiveEntry::new_file("a.txt");
        writer
            .push_archive_entry(e1, Some(std::io::Cursor::new(b"a")))
            .unwrap();
        let e2 = sevenz_rust2::ArchiveEntry::new_directory("dir1");
        writer
            .push_archive_entry(e2, None::<std::io::Cursor<Vec<u8>>>)
            .unwrap();
        let e3 = sevenz_rust2::ArchiveEntry::new_file("dir1/b.txt");
        writer
            .push_archive_entry(e3, Some(std::io::Cursor::new(b"b")))
            .unwrap();
        writer.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let entries = archive_fs.list_dir(Path::new(".")).unwrap();

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"a.txt"),
        "root should contain a.txt, got {names:?}"
    );
    assert!(
        names.contains(&"dir1"),
        "root should contain dir1, got {names:?}"
    );

    let sub_entries = archive_fs.list_dir(Path::new("dir1")).unwrap();
    let sub_names: Vec<&str> = sub_entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        sub_names.contains(&"b.txt"),
        "dir1 should contain b.txt, got {sub_names:?}"
    );
}

#[tokio::test]
async fn test_7z_copy_from_local() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();
    std::fs::write(src_dir.join("file1.txt"), b"content1").unwrap();
    let sub = src_dir.join("subdir");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("file2.txt"), b"content2").unwrap();

    let archive_path = temp_dir.path().join("test_copy.7z");
    create_empty_7z(&archive_path);

    let src_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    let progress = make_progress();

    let result = archive_fs
        .copy_from_local(&src_fs, &src_dir, Path::new("copied"), &progress)
        .await
        .unwrap();
    result.unwrap();

    assert!(
        archive_fs
            .get_entry(Path::new("copied/file1.txt"))
            .is_some()
    );
    assert!(
        archive_fs
            .get_entry(Path::new("copied/subdir/file2.txt"))
            .is_some()
    );

    let data = archive_fs.read_file(Path::new("copied/file1.txt")).unwrap();
    assert_eq!(data, b"content1");
}

#[tokio::test]
async fn test_7z_add_files_batch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src_dir");
    std::fs::create_dir(&src_dir).unwrap();
    std::fs::write(src_dir.join("file1.txt"), b"content1").unwrap();
    std::fs::write(src_dir.join("file2.txt"), b"content2").unwrap();
    let sub_dir = src_dir.join("subdir");
    std::fs::create_dir(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("file3.txt"), b"content3").unwrap();

    let archive_path = temp_dir.path().join("test_batch.7z");
    create_empty_7z(&archive_path);

    let src_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    let progress = make_progress();

    archive_fs
        .copy_from_local(&src_fs, &src_dir, Path::new("batch_dir"), &progress)
        .await
        .unwrap()
        .unwrap();

    assert!(
        archive_fs
            .get_entry(Path::new("batch_dir/file1.txt"))
            .is_some()
    );
    assert!(
        archive_fs
            .get_entry(Path::new("batch_dir/file2.txt"))
            .is_some()
    );
    assert!(
        archive_fs
            .get_entry(Path::new("batch_dir/subdir/file3.txt"))
            .is_some()
    );

    let data1 = archive_fs
        .read_file(Path::new("batch_dir/file1.txt"))
        .unwrap();
    assert_eq!(data1, b"content1");
    let data3 = archive_fs
        .read_file(Path::new("batch_dir/subdir/file3.txt"))
        .unwrap();
    assert_eq!(data3, b"content3");
}

#[tokio::test]
async fn test_7z_copy_directory_tree_batch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src = temp_dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("top.txt"), b"top").unwrap();
    let sub1 = src.join("sub1");
    std::fs::create_dir(&sub1).unwrap();
    std::fs::write(sub1.join("a.txt"), b"a").unwrap();
    let sub1a = sub1.join("sub1a");
    std::fs::create_dir(&sub1a).unwrap();
    std::fs::write(sub1a.join("b.txt"), b"b").unwrap();
    let sub2 = src.join("sub2");
    std::fs::create_dir(&sub2).unwrap();
    std::fs::write(sub2.join("c.txt"), b"c").unwrap();

    let archive_path = temp_dir.path().join("test_tree.7z");
    create_empty_7z(&archive_path);

    let src_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    let progress = make_progress();

    archive_fs
        .copy_from_local(&src_fs, &src, Path::new("tree"), &progress)
        .await
        .unwrap()
        .unwrap();

    for path in [
        "tree/top.txt",
        "tree/sub1",
        "tree/sub1/a.txt",
        "tree/sub1/sub1a",
        "tree/sub1/sub1a/b.txt",
        "tree/sub2",
        "tree/sub2/c.txt",
    ] {
        assert!(
            archive_fs.get_entry(Path::new(path)).is_some(),
            "{path} missing"
        );
    }

    let data = archive_fs
        .read_file(Path::new("tree/sub1/sub1a/b.txt"))
        .unwrap();
    assert_eq!(data, b"b");
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
            .unwrap(),
        b"inner"
    );
}

#[test]
fn test_7z_single_file_has_no_directory_size() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_meta.7z");

    {
        let mut writer = ArchiveWriter::create(&archive_path).unwrap();
        let mut entry = sevenz_rust2::ArchiveEntry::new_file("f.txt");
        entry.last_modified_date = sevenz_rust2::NtTime::now();
        entry.has_last_modified_date = true;
        writer
            .push_archive_entry(entry, Some(std::io::Cursor::new(b"data")))
            .unwrap();
        writer.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let entry = archive_fs.get_entry(Path::new("f.txt")).unwrap();
    assert!(!entry.file_entry.is_dir);
    assert_eq!(entry.file_entry.size, Some(4));
    assert!(entry.file_entry.modified.is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn test_7z_scan_shows_unix_mode_from_attributes() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_mode_scan.7z");

    {
        let mut writer = ArchiveWriter::create(&archive_path).unwrap();
        let mut script = sevenz_rust2::ArchiveEntry::new_file("run.sh");
        script.has_windows_attributes = true;
        script.windows_attributes = 0o100_755;
        writer
            .push_archive_entry(script, Some(std::io::Cursor::new(b"#!/bin/sh\n")))
            .unwrap();
        let mut plain = sevenz_rust2::ArchiveEntry::new_file("plain.txt");
        plain.has_windows_attributes = true;
        plain.windows_attributes = 0o100_644;
        writer
            .push_archive_entry(plain, Some(std::io::Cursor::new(b"content")))
            .unwrap();
        let mut dir = sevenz_rust2::ArchiveEntry::new_directory("scripts");
        dir.has_windows_attributes = true;
        dir.windows_attributes = 0o040_755;
        writer
            .push_archive_entry(dir, None::<std::io::Cursor<Vec<u8>>>)
            .unwrap();
        writer.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    assert_eq!(
        archive_fs
            .get_entry(Path::new("run.sh"))
            .unwrap()
            .file_entry
            .attributes,
        "-rwxr-xr-x"
    );
    assert_eq!(
        archive_fs
            .get_entry(Path::new("plain.txt"))
            .unwrap()
            .file_entry
            .attributes,
        "-rw-r--r--"
    );
    assert_eq!(
        archive_fs
            .get_entry(Path::new("scripts"))
            .unwrap()
            .file_entry
            .attributes,
        "drwxr-xr-x"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_7z_add_preserves_executable_permission() {
    use std::os::unix::fs::PermissionsExt;
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();
    let script = src_dir.join("run.sh");
    std::fs::write(&script, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let plain = src_dir.join("plain.txt");
    std::fs::write(&plain, b"content").unwrap();
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();

    let archive_path = temp_dir.path().join("test_mode_add.7z");
    create_empty_7z(&archive_path);

    let src_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let progress = make_progress();

    archive_fs
        .copy_from_local(&src_fs, &src_dir, Path::new("bin"), &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        archive_fs
            .get_entry(Path::new("bin/run.sh"))
            .unwrap()
            .file_entry
            .attributes,
        "-rwxr-xr-x"
    );
    assert_eq!(
        archive_fs
            .get_entry(Path::new("bin/plain.txt"))
            .unwrap()
            .file_entry
            .attributes,
        "-rw-r--r--"
    );

    // The stored value must carry the unix file-type bits, like p7zip does.
    let reader = ArchiveReader::open(&archive_path, Password::empty()).unwrap();
    let script = reader
        .archive()
        .files
        .iter()
        .find(|f| f.name() == "bin/run.sh")
        .unwrap();
    assert!(script.has_windows_attributes);
    assert_eq!(script.windows_attributes & 0o777, 0o755);
    assert_eq!(script.windows_attributes & 0o177_777, 0o100_755);
}

#[cfg(unix)]
#[tokio::test]
async fn test_7z_extract_restores_executable_permission() {
    use std::os::unix::fs::PermissionsExt;
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();
    let script = src_dir.join("run.sh");
    std::fs::write(&script, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let archive_path = temp_dir.path().join("test_mode_extract.7z");
    create_empty_7z(&archive_path);

    let src_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let progress = make_progress();

    archive_fs
        .copy_from_local(&src_fs, &src_dir, Path::new("bin"), &progress)
        .await
        .unwrap()
        .unwrap();

    let extract_dir = temp_dir.path().join("extracted");
    std::fs::create_dir(&extract_dir).unwrap();
    let dest_fs = ProviderFileSystem(std::sync::Arc::new(LocalFs::new()));

    let result = archive_fs
        .extract(Path::new("."), &dest_fs, &extract_dir, &progress)
        .await
        .unwrap();
    result.unwrap();

    let meta = std::fs::metadata(extract_dir.join("bin/run.sh")).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o755);
    assert_ne!(meta.permissions().mode() & 0o100, 0);
}
