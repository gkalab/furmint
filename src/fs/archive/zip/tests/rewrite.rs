use super::*;

#[tokio::test]
async fn test_zip_add_files_batch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src_dir");
    std::fs::create_dir(&src_dir).unwrap();
    std::fs::write(src_dir.join("file1.txt"), b"content1").unwrap();
    std::fs::write(src_dir.join("file2.txt"), b"content2").unwrap();
    let sub_dir = src_dir.join("subdir");
    std::fs::create_dir(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("file3.txt"), b"content3").unwrap();

    let archive_path = temp_dir.path().join("test_batch.zip");
    {
        let file = File::create(&archive_path).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();
    }

    let src_fs = LocalFs::new();
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();

    let progress = crate::fs::provider::TaskProgressContext {
        id: 1,
        tx: crate::tasks::EventBus::new(tokio::sync::mpsc::unbounded_channel().0),
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };

    // This should trigger the recursive copy_from_local optimization
    archive_fs
        .copy_from_local(&src_fs, &src_dir, Path::new("batch_dir"), &progress)
        .await
        .unwrap()
        .unwrap();

    // Verify contents
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

    // Verify data
    let data1 = archive_fs
        .read_file(Path::new("batch_dir/file1.txt"))
        .await
        .unwrap();
    assert_eq!(data1, b"content1");
    let data3 = archive_fs
        .read_file(Path::new("batch_dir/subdir/file3.txt"))
        .await
        .unwrap();
    assert_eq!(data3, b"content3");
}

/// Entry names in the raw zip must use forward slashes, otherwise the archive
/// is not portable (`normalize_path` would leak `\` on Windows).
#[tokio::test]
async fn test_zip_batch_add_uses_forward_slashes() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src_dir");
    let sub_dir = src_dir.join("subdir");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("file.txt"), b"content").unwrap();

    let archive_path = temp_dir.path().join("slashes.zip");
    {
        let file = File::create(&archive_path).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let progress = crate::fs::provider::TaskProgressContext {
        id: 1,
        tx: crate::tasks::EventBus::new(tokio::sync::mpsc::unbounded_channel().0),
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    archive_fs
        .copy_from_local(&LocalFs::new(), &src_dir, Path::new("top"), &progress)
        .await
        .unwrap()
        .unwrap();

    let file = File::open(&archive_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["top/", "top/subdir/", "top/subdir/file.txt"]);
}

/// Builds a zip holding `entries` as `(name, compression method, contents)`.
fn write_archive(path: &Path, entries: &[(&str, CompressionMethod, &[u8])]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, method, data) in entries {
        if name.ends_with('/') {
            zip.add_directory(
                *name,
                SimpleFileOptions::default().compression_method(*method),
            )
            .unwrap();
        } else {
            let options = SimpleFileOptions::default().compression_method(*method);
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
    }
    zip.finish().unwrap();
}

/// Entry names in the archive, sorted, so duplicates show up.
fn entry_names(path: &Path) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    let mut names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    names.sort();
    names
}

fn entry_method(path: &Path, name: &str) -> CompressionMethod {
    let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    archive.by_name(name).unwrap().compression()
}

fn read_entry(path: &Path, name: &str) -> Vec<u8> {
    ZipHandler::new(path).read_file(name).unwrap()
}

/// Touching an entry must not re-encode it: a STORED entry stays STORED, so the
/// stored bytes are reused instead of being run through the deflate encoder.
#[test]
fn test_zip_set_modified_time_preserves_compression() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("touch.zip");
    write_archive(
        &path,
        &[
            ("stored.bin", CompressionMethod::Stored, b"stored payload"),
            (
                "deflated.txt",
                CompressionMethod::Deflated,
                b"deflated payload",
            ),
        ],
    );

    let handler = ZipHandler::new(&path);
    let new_mtime = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    handler.set_modified_time("stored.bin", new_mtime).unwrap();
    handler
        .set_modified_time("deflated.txt", new_mtime)
        .unwrap();

    assert_eq!(entry_method(&path, "stored.bin"), CompressionMethod::Stored);
    assert_eq!(
        entry_method(&path, "deflated.txt"),
        CompressionMethod::Deflated
    );
    assert_eq!(read_entry(&path, "stored.bin"), b"stored payload");
    assert_eq!(read_entry(&path, "deflated.txt"), b"deflated payload");

    let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
    let stamp = archive
        .by_name("stored.bin")
        .unwrap()
        .last_modified()
        .unwrap();
    let expected = zip::DateTime::from_date_and_time(2001, 9, 9, 1, 46, 40).unwrap();
    assert_eq!(stamp, expected);
}

/// Renaming must reuse the original compressed bytes, keeping both the
/// compression method and the directory marker intact.
#[test]
fn test_zip_rename_preserves_compression() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("rename.zip");
    write_archive(
        &path,
        &[
            ("old/", CompressionMethod::Stored, b""),
            (
                "old/stored.bin",
                CompressionMethod::Stored,
                b"stored payload",
            ),
            (
                "old/deflated.txt",
                CompressionMethod::Deflated,
                b"deflated payload",
            ),
        ],
    );

    ZipHandler::new(&path).rename_file("old", "new").unwrap();

    assert_eq!(
        entry_names(&path),
        vec!["new/", "new/deflated.txt", "new/stored.bin",]
    );
    assert_eq!(
        entry_method(&path, "new/stored.bin"),
        CompressionMethod::Stored
    );
    assert_eq!(
        entry_method(&path, "new/deflated.txt"),
        CompressionMethod::Deflated
    );
    assert_eq!(read_entry(&path, "new/stored.bin"), b"stored payload");
    assert_eq!(read_entry(&path, "new/deflated.txt"), b"deflated payload");
}

/// Replacing a destination rewrites the archive exactly once: the stale entry
/// and its replacement are emitted in the same pass, so the archive must end up
/// with a single entry per destination and the untouched entries intact.
#[test]
fn test_zip_replace_entry_single_pass() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("replace.zip");
    write_archive(
        &path,
        &[
            ("keep/", CompressionMethod::Stored, b""),
            ("keep/other.txt", CompressionMethod::Deflated, b"other"),
            ("target/", CompressionMethod::Stored, b""),
            ("target.txt", CompressionMethod::Stored, b"stale"),
        ],
    );

    let new_src = temp_dir.path().join("upload");
    std::fs::write(&new_src, b"fresh").unwrap();

    let handler = ZipHandler::new(&path);
    handler.add_file(&new_src, "target.txt").unwrap();
    handler.add_directory("target", None).unwrap();

    assert_eq!(
        entry_names(&path),
        vec!["keep/", "keep/other.txt", "target.txt", "target/",]
    );
    assert_eq!(read_entry(&path, "target.txt"), b"fresh");
    assert_eq!(read_entry(&path, "keep/other.txt"), b"other");
    assert_eq!(
        entry_method(&path, "keep/other.txt"),
        CompressionMethod::Deflated
    );
}

/// The same holds for the batched entry point, which mixes replacement and
/// fresh destinations in one call.
#[test]
fn test_zip_batch_replace_entry_single_pass() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("batch_replace.zip");
    write_archive(
        &path,
        &[
            ("a.txt", CompressionMethod::Stored, b"old a"),
            ("b.txt", CompressionMethod::Stored, b"old b"),
        ],
    );

    let src_a = temp_dir.path().join("a");
    let src_c = temp_dir.path().join("c");
    std::fs::write(&src_a, b"new a").unwrap();
    std::fs::write(&src_c, b"new c").unwrap();

    ZipHandler::new(&path)
        .add_files(&[(&src_a, "a.txt"), (&src_c, "c.txt"), (&src_c, "b.txt")])
        .unwrap();

    assert_eq!(entry_names(&path), vec!["a.txt", "b.txt", "c.txt"]);
    assert_eq!(read_entry(&path, "a.txt"), b"new a");
    assert_eq!(read_entry(&path, "b.txt"), b"new c");
    assert_eq!(read_entry(&path, "c.txt"), b"new c");
}
