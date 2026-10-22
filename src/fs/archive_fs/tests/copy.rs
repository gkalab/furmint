use super::*;

/// Copying one file out of a tar.gz must count it exactly once. The tar
/// handler reports the entry it extracts, so the walker must not also add the
/// top-level file, which used to render the task line at 200%.
#[tokio::test]
async fn test_copy_single_file_from_tar_gz_counts_item_once() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("src.tar.gz");
    make_tar_gz(&src_path, &[("a.txt", b"hello")]);

    let dest_dir = temp_dir.path().join("out");
    let dest = dest_dir.join("a.txt");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let src_fs = ArchiveFs::new(&src_path).unwrap();
    let dest_fs = LocalFs::new();

    let processed = recursive_copy(&src_fs, &dest_fs, Path::new("a.txt"), &dest, 1)
        .await
        .unwrap();

    assert_eq!(
        processed, 1,
        "the file was counted twice, so the task line overshoots 100%"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
}

/// A whole directory extracted from an archive must be credited as one
/// subtree by the walker. The extractor reports no items, so this checks the
/// delegation credit lands on the task total instead of stalling below it.
#[tokio::test]
async fn test_copy_directory_from_archive_counts_subtree_once() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("src.zip");
    make_archive(
        &src_path,
        &[("tree/top.txt", b"top"), ("tree/sub/deep.txt", b"deep")],
    );

    let dest_dir = temp_dir.path().join("out");
    let dest = dest_dir.join("tree");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let src_fs = ArchiveFs::new(&src_path).unwrap();
    let dest_fs = LocalFs::new();

    // tree + top.txt + sub + deep.txt
    let processed = recursive_copy(&src_fs, &dest_fs, Path::new("tree"), &dest, 4)
        .await
        .unwrap();

    assert_eq!(
        processed, 4,
        "the extracted subtree must be credited exactly once"
    );
    assert_eq!(std::fs::read(dest.join("top.txt")).unwrap(), b"top");
    assert_eq!(std::fs::read(dest.join("sub/deep.txt")).unwrap(), b"deep");
}

#[tokio::test]
async fn test_copy_file_between_two_archives() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("src.zip");
    let dest_path = temp_dir.path().join("dest.zip");
    make_archive(&src_path, &[("a.txt", b"hello")]);
    make_archive(&dest_path, &[]);

    let src_fs = ArchiveFs::new(&src_path).unwrap();
    let dest_fs = ArchiveFs::new(&dest_path).unwrap();

    recursive_copy(&src_fs, &dest_fs, Path::new("a.txt"), Path::new("a.txt"), 1)
        .await
        .unwrap();

    assert!(dest_fs.exists(Path::new("a.txt")).await);
    assert_eq!(
        dest_fs.read_file(Path::new("a.txt")).await.unwrap(),
        b"hello"
    );
}

#[tokio::test]
async fn test_copy_directory_between_two_archives() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("src.zip");
    let dest_path = temp_dir.path().join("dest.zip");
    make_archive(
        &src_path,
        &[("tree/top.txt", b"top"), ("tree/sub/deep.txt", b"deep")],
    );
    make_archive(&dest_path, &[("existing.txt", b"keep")]);

    let src_fs = ArchiveFs::new(&src_path).unwrap();
    let dest_fs = ArchiveFs::new(&dest_path).unwrap();

    // tree + top.txt + sub + deep.txt
    recursive_copy(&src_fs, &dest_fs, Path::new("tree"), Path::new("tree"), 4)
        .await
        .unwrap();

    for path in ["tree/top.txt", "tree/sub/deep.txt"] {
        assert!(dest_fs.exists(Path::new(path)).await, "{path} missing");
    }
    assert_eq!(
        dest_fs
            .read_file(Path::new("tree/sub/deep.txt"))
            .await
            .unwrap(),
        b"deep"
    );
    // Pre-existing entries survive.
    assert_eq!(
        dest_fs.read_file(Path::new("existing.txt")).await.unwrap(),
        b"keep"
    );
}

#[tokio::test]
async fn test_copy_directory_between_two_7z_archives() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("src.7z");
    let dest_path = temp_dir.path().join("dest.7z");
    {
        let file = File::create(&src_path).unwrap();
        let mut writer = sevenz_rust2::ArchiveWriter::new(file).unwrap();
        let entry = sevenz_rust2::ArchiveEntry::new_file("tree/one.txt");
        let mut data: &[u8] = b"seven";
        writer.push_archive_entry(entry, Some(&mut data)).unwrap();
        writer.finish().unwrap();
    }
    let local = LocalFs::new();
    local.create_file(&dest_path).await.unwrap();

    let src_fs = ArchiveFs::new(&src_path).unwrap();
    let dest_fs = ArchiveFs::new(&dest_path).unwrap();

    recursive_copy(&src_fs, &dest_fs, Path::new("tree"), Path::new("tree"), 2)
        .await
        .unwrap();

    assert_eq!(
        dest_fs.read_file(Path::new("tree/one.txt")).await.unwrap(),
        b"seven"
    );
}

/// Archive entry names must use forward slashes so the archives stay readable
/// on other platforms (`normalize_path` would leak `\` on Windows).
#[tokio::test]
async fn test_copied_entries_use_forward_slashes() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("slashes_src.zip");
    let dest_path = temp_dir.path().join("slashes_dest.zip");
    make_archive(&src_path, &[("a/b/c.txt", b"nested")]);
    make_archive(&dest_path, &[]);

    let src_fs = ArchiveFs::new(&src_path).unwrap();
    let dest_fs = ArchiveFs::new(&dest_path).unwrap();

    recursive_copy(&src_fs, &dest_fs, Path::new("a"), Path::new("x"), 2)
        .await
        .unwrap();

    let file = File::open(&dest_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["x/", "x/b/", "x/b/c.txt"]);
}

#[tokio::test]
async fn test_copy_directory_between_zip_and_7z() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("src.zip");
    let dest_path = temp_dir.path().join("dest.7z");
    make_archive(&src_path, &[("tree/top.txt", b"top")]);
    LocalFs::new().create_file(&dest_path).await.unwrap();

    let src_fs = ArchiveFs::new(&src_path).unwrap();
    let dest_fs = ArchiveFs::new(&dest_path).unwrap();

    recursive_copy(&src_fs, &dest_fs, Path::new("tree"), Path::new("tree"), 2)
        .await
        .unwrap();

    assert_eq!(
        dest_fs.read_file(Path::new("tree/top.txt")).await.unwrap(),
        b"top"
    );
}

/// Copying a local directory reached through a symlink into a zip must succeed.
/// The walk has to follow the link, otherwise its root is yielded as a link
/// rather than a directory and lands in the file list under the destination's
/// own name, which makes the batch add fail.
#[cfg(unix)]
#[tokio::test]
async fn test_copy_local_symlinked_directory_into_zip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let real = temp_dir.path().join("realdir");
    std::fs::create_dir_all(real.join("sub")).unwrap();
    std::fs::write(real.join("top.txt"), b"top").unwrap();
    std::fs::write(real.join("sub/deep.txt"), b"deep").unwrap();

    let link = temp_dir.path().join("linkdir");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let zip_path = temp_dir.path().join("dest.zip");
    make_archive(&zip_path, &[]);
    let dest_fs = ArchiveFs::new(&zip_path).unwrap();
    let local_fs = LocalFs::new();

    let (raw_tx, mut rx) = mpsc::unbounded_channel::<UiEvent>();
    let bus = crate::tasks::EventBus::new(raw_tx);
    let progress = TaskProgressContext {
        id: 1,
        tx: bus,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };

    dest_fs
        .copy_from_local(&local_fs, &link, Path::new("linkdir"), &progress)
        .await
        .expect("archive copies from a local source")
        .expect("copying a linked directory must succeed");

    while let Ok(ev) = rx.try_recv() {
        if let UiEvent::Alert(AlertEvent::TaskError { message, .. }) = ev {
            panic!("copy reported an error: {message}");
        }
    }

    assert_eq!(
        dest_fs
            .read_file(Path::new("linkdir/top.txt"))
            .await
            .unwrap(),
        b"top"
    );
    assert_eq!(
        dest_fs
            .read_file(Path::new("linkdir/sub/deep.txt"))
            .await
            .unwrap(),
        b"deep"
    );
}
