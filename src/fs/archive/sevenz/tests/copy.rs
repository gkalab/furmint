use super::*;

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
    let dest_fs = LocalFs::new();

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

    let src_fs = LocalFs::new();
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

    let data = archive_fs
        .read_file(Path::new("copied/file1.txt"))
        .await
        .unwrap();
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

    let src_fs = LocalFs::new();
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
        .await
        .unwrap();
    assert_eq!(data1, b"content1");
    let data3 = archive_fs
        .read_file(Path::new("batch_dir/subdir/file3.txt"))
        .await
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

    let src_fs = LocalFs::new();
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
        .await
        .unwrap();
    assert_eq!(data, b"b");
}
