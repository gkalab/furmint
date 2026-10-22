use super::*;

#[tokio::test]
async fn test_archive_fs_read_and_download_zip() {
    // 1. Setup ZIP with a file and a subdirectory
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("hello.txt", options).unwrap();
        zip.write_all(b"world").unwrap();

        zip.add_directory("dir", options).unwrap();
        zip.start_file("dir/sub.txt", options).unwrap();
        zip.write_all(b"subordinate").unwrap();

        zip.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();

    // 3. Test read_file
    let content = archive_fs.read_file(Path::new("hello.txt")).await.unwrap();
    assert_eq!(content, b"world");

    let sub_content = archive_fs
        .read_file(Path::new("dir/sub.txt"))
        .await
        .unwrap();
    assert_eq!(sub_content, b"subordinate");

    // 4. Test download (optimized extraction)
    let dest_dir = temp_dir.path().join("extracted_zip");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = crate::fs::provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    // Extract everything
    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("hello.txt")).unwrap(),
        "world"
    );
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("dir/sub.txt")).unwrap(),
        "subordinate"
    );
}

#[tokio::test]
async fn test_archive_fs_read_and_download_tar_gz() {
    // 1. Setup TAR.GZ
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.gz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut tar = tar::Builder::new(enc);

        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(5);
        tar.append_data(&mut header, "hello.txt", b"world" as &[u8])
            .unwrap();

        let mut header2 = tar::Header::new_gnu();
        header2.set_mode(0o644);
        header2.set_size(11);
        tar.append_data(&mut header2, "dir/sub.txt", b"subordinate" as &[u8])
            .unwrap();

        tar.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();

    // 3. Test read_file
    let content = archive_fs.read_file(Path::new("hello.txt")).await.unwrap();
    assert_eq!(content, b"world");

    // 4. Test download
    let dest_dir = temp_dir.path().join("extracted_tar");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = crate::fs::provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("hello.txt")).unwrap(),
        "world"
    );
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("dir/sub.txt")).unwrap(),
        "subordinate"
    );
}

#[tokio::test]
async fn test_archive_fs_read_and_download_plain_gz() {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.gz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut writer = std::io::BufWriter::new(enc);
        writer.write_all(b"hello world").unwrap();
        writer.flush().unwrap();
    }

    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();

    let content = archive_fs.read_file(Path::new("test")).await.unwrap();
    assert_eq!(content, b"hello world");

    let dest_dir = temp_dir.path().join("extracted_gz");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = crate::fs::provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("test")).unwrap(),
        "hello world"
    );
}

#[tokio::test]
async fn test_archive_fs_plain_gz_attributes() {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.gz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut writer = std::io::BufWriter::new(enc);
        writer.write_all(b"test content").unwrap();
        writer.flush().unwrap();
    }

    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();

    let entry = archive_fs.get_entry(Path::new("test")).unwrap();
    assert_eq!(entry.name, "test");
    assert_eq!(entry.size, Some(12));
    assert!(!entry.is_dir);
    assert!(!entry.is_symlink);
}

#[tokio::test]
async fn test_archive_download_empty_dir_and_nesting() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("nesting.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        // Empty directory
        zip.add_directory("empty_dir", options).unwrap();

        // Nested structure
        zip.add_directory("a", options).unwrap();
        zip.add_directory("a/b", options).unwrap();
        zip.start_file("a/b/c.txt", options).unwrap();
        zip.write_all(b"nested content").unwrap();

        zip.finish().unwrap();
    }

    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();
    let dest_dir = temp_dir.path().join("extracted_nesting");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = crate::fs::provider::TaskProgressContext {
        id: 123,
        tx: tx.clone(),
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    // Verify files
    assert!(dest_dir.join("empty_dir").is_dir());
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("a/b/c.txt")).unwrap(),
        "nested content"
    );
}

#[tokio::test]
async fn test_archive_download_cancellation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("cancel.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        // Create many files to ensure we have time to cancel
        for i in 0..1000 {
            zip.start_file(format!("file_{i}.txt"), options).unwrap();
            zip.write_all(b"some data").unwrap();
        }

        zip.finish().unwrap();
    }

    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();
    let dest_dir = temp_dir.path().join("extracted_cancel");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let progress = crate::fs::provider::TaskProgressContext {
        id: 789,
        tx,
        cancel: cancel_flag.clone(),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    // Cancel after a short delay
    let cancel_flag_clone = cancel_flag.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        cancel_flag_clone.store(true, Ordering::SeqCst);
    });

    let res = archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap();

    // archive_fs::download returns Ok(()) on cancellation
    assert!(res.is_ok());

    // Should not have extracted all 1000 files
    let count = std::fs::read_dir(&dest_dir).unwrap().count();
    assert!(count < 1000);
}

#[tokio::test]
async fn test_archive_fs_download_tar_gz() {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    // 1. Setup TAR.GZ with a file
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.gz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(enc);

        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(5);
        header.set_cksum();
        builder
            .append_data(&mut header, "hello.txt", "world".as_bytes())
            .unwrap();

        builder.finish().unwrap();
    }

    // 2. Load ArchiveFs (this will decompress to a temp tar)
    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();

    assert!(
        archive_fs.get_entry(Path::new("hello.txt")).is_some(),
        "entry not found"
    );

    // 3. Test download (extraction from the temp tar)
    let dest_file = temp_dir.path().join("extracted_hello.txt");

    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = crate::fs::provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    // Extract single file
    archive_fs
        .extract(Path::new("hello.txt"), &local_fs, &dest_file, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(std::fs::read_to_string(&dest_file).unwrap(), "world");
}

#[tokio::test]
async fn test_archive_fs_read_and_download_xz() {
    // 1. Setup .tar.xz
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.xz");

    {
        let file = File::create(&archive_path).unwrap();
        let enc = xz2::write::XzEncoder::new(file, 6);
        let mut builder = tar::Builder::new(enc);

        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(11);
        header.set_cksum();
        builder
            .append_data(&mut header, "hello_xz.txt", "world of xz".as_bytes())
            .unwrap();

        builder.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();

    // 3. Test read_file
    let content = archive_fs
        .read_file(Path::new("hello_xz.txt"))
        .await
        .unwrap();
    assert_eq!(content, b"world of xz");

    // 4. Test extraction
    let dest_dir = temp_dir.path().join("extracted_xz");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = crate::fs::provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("hello_xz.txt")).unwrap(),
        "world of xz"
    );
}

#[tokio::test]
async fn test_archive_fs_read_and_download_rpm() {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let temp_dir = tempfile::tempdir().unwrap();
    let rpm_path = temp_dir.path().join("test.rpm");

    // Header structure helper
    let write_header = |f: &mut File, tags: Vec<(i32, i32, &[u8])>| {
        f.write_all(b"\x8e\xad\xe8\x01\x00\x00\x00\x00").unwrap(); // magic + version + reserved
        f.write_all(&(u32::try_from(tags.len()).unwrap()).to_be_bytes())
            .unwrap(); // count

        let mut data = Vec::new();
        let mut index = Vec::new();

        for (tag, ty, val) in tags {
            let offset = i32::try_from(data.len()).unwrap();
            index.push((tag, ty, offset, 1i32));
            data.extend_from_slice(val);
            if ty == 6 || ty == 9 {
                data.push(0);
            }
        }

        f.write_all(&(u32::try_from(data.len()).unwrap()).to_be_bytes())
            .unwrap(); // size

        for (tag, ty, offset, cnt) in index {
            f.write_all(&tag.to_be_bytes()).unwrap();
            f.write_all(&ty.to_be_bytes()).unwrap();
            f.write_all(&offset.to_be_bytes()).unwrap();
            f.write_all(&cnt.to_be_bytes()).unwrap();
        }

        f.write_all(&data).unwrap();
    };

    // Create a dummy RPM
    {
        let mut file = File::create(&rpm_path).unwrap();
        // Lead: 96 bytes
        file.write_all(&[0u8; 96]).unwrap();

        // Signature
        write_header(&mut file, vec![]);
        // Padding to 8-byte boundary
        let pos = file.stream_position().unwrap();
        let pad = (8 - (pos % 8)) % 8;
        file.write_all(&vec![0u8; pad as usize]).unwrap();

        // Main Header
        write_header(
            &mut file,
            vec![(1000, 6, b"test-package"), (1001, 6, b"1.2.3")],
        );

        // Payload (uncompressed CPIO)
        let builder = cpio::NewcBuilder::new("file1.txt")
            .mode(0o100_644)
            .mtime(1000);
        let data = b"hello rpm";
        let mut writer = builder.write(file, u32::try_from(data.len()).unwrap());
        writer.write_all(data).unwrap();
        let mut file = writer.finish().unwrap();

        // Trailer
        cpio::newc::trailer(&mut file).unwrap();
    }

    // Test metadata (unsigned)
    let handler = crate::fs::archive::rpm::RpmHandler::new(&rpm_path);
    let meta = handler.get_metadata().unwrap();
    assert!(meta.contains("Signed         : no"));
    assert!(meta.contains("Name           : test-package"));
    assert!(meta.contains("Version        : 1.2.3"));

    // Create a signed RPM
    let signed_rpm_path = temp_dir.path().join("signed.rpm");
    {
        let mut file = File::create(&signed_rpm_path).unwrap();
        file.write_all(&[0u8; 96]).unwrap();

        // Signature header with RPMSIGTAG_PGP (1002)
        write_header(&mut file, vec![(1002, 7, b"signature-data")]);
        let pos = file.stream_position().unwrap();
        let pad = (8 - (pos % 8)) % 8;
        file.write_all(&vec![0u8; pad as usize]).unwrap();

        write_header(&mut file, vec![(1000, 6, b"signed-package")]);
        cpio::newc::trailer(&mut file).unwrap();
    }

    let handler_signed = crate::fs::archive::rpm::RpmHandler::new(&signed_rpm_path);
    let meta_signed = handler_signed.get_metadata().unwrap();
    assert!(meta_signed.contains("Signed         : yes"));

    // Test scanning
    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&rpm_path).unwrap();
    let entries = archive_fs.list_dir(Path::new("/")).await.unwrap();
    assert!(entries.iter().any(|e| e.name == "file1.txt"));

    // Test reading
    let content = archive_fs.read_file(Path::new("file1.txt")).await.unwrap();
    assert_eq!(content, b"hello rpm");

    // Test extraction
    let dest_dir = temp_dir.path().join("extracted_rpm");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<crate::tasks::UiEvent>();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = crate::fs::provider::TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };
    let local_fs = LocalFs::new();

    archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dest_dir.join("file1.txt")).unwrap(),
        "hello rpm"
    );
}
