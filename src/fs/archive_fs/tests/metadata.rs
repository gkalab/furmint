use super::*;

#[tokio::test]
async fn test_zip_extract_attributes() {
    // 1. Setup ZIP with a file and a subdirectory
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_attributes.zip");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        // File with specific permissions
        let mut options = zip::write::SimpleFileOptions::default();
        options = options.unix_permissions(0o755); // rwxr-xr-x
        zip.start_file("executable.sh", options).unwrap();
        zip.write_all(b"#!/bin/bash\necho hello").unwrap();

        // Directory with specific permissions
        let mut dir_options = zip::write::SimpleFileOptions::default();
        dir_options = dir_options.unix_permissions(0o700); // rwx------
        zip.add_directory("private_dir", dir_options).unwrap();

        // File inside the private directory
        let mut sub_file_options = zip::write::SimpleFileOptions::default();
        sub_file_options = sub_file_options.unix_permissions(0o640); // rw-r-----
        zip.start_file("private_dir/secret.txt", sub_file_options)
            .unwrap();
        zip.write_all(b"top secret").unwrap();

        zip.finish().unwrap();
    }

    // 2. Load ArchiveFs
    let archive_fs = crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap();

    // 3. Test download (extraction with attribute preservation)
    let dest_dir = temp_dir.path().join("extracted_zip_attributes");
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

    // Verify file content
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("executable.sh")).unwrap(),
        "#!/bin/bash\necho hello"
    );
    assert_eq!(
        std::fs::read_to_string(dest_dir.join("private_dir/secret.txt")).unwrap(),
        "top secret"
    );

    // Verify permissions (Unix-like systems only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let executable_path = dest_dir.join("executable.sh");
        let executable_metadata = std::fs::metadata(&executable_path).unwrap();
        assert_eq!(executable_metadata.permissions().mode() & 0o777, 0o755);

        let private_dir_path = dest_dir.join("private_dir");
        let private_dir_metadata = std::fs::metadata(&private_dir_path).unwrap();
        assert_eq!(private_dir_metadata.permissions().mode() & 0o777, 0o700);

        let secret_file_path = dest_dir.join("private_dir/secret.txt");
        let secret_file_metadata = std::fs::metadata(&secret_file_path).unwrap();
        assert_eq!(secret_file_metadata.permissions().mode() & 0o777, 0o640);
    }
}

#[tokio::test]
async fn test_zip_timestamp_preservation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.zip");
    let dest_dir = temp_dir.path().join("extracted");

    {
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let dt = zip::DateTime::from_date_and_time(2025, 1, 1, 12, 0, 0).unwrap();
        let options = zip::write::SimpleFileOptions::default().last_modified_time(dt);

        zip.add_directory("dir", options).unwrap();
        zip.start_file("dir/file.txt", options).unwrap();
        zip.write_all(b"content").unwrap();
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let local_fs = LocalFs::new();
    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = TaskProgressContext {
        id: 0,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };

    let result: anyhow::Result<()> = archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap();
    result.unwrap();

    // Verify file timestamp
    let file_meta = std::fs::metadata(dest_dir.join("dir/file.txt")).unwrap();
    let file_mtime = file_meta.modified().unwrap();

    // Verify directory timestamp
    let dir_meta = std::fs::metadata(dest_dir.join("dir")).unwrap();
    let dir_mtime = dir_meta.modified().unwrap();

    // Convert SystemTime to chrono for easier comparison if needed, or just compare
    // Note: ZIP has 2-second resolution.
    let expected = SystemTime::from(chrono::TimeZone::from_utc_datetime(
        &chrono::Utc,
        &chrono::NaiveDate::from_ymd_opt(2025, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
    ));

    let diff_file = if file_mtime > expected {
        file_mtime.duration_since(expected).unwrap()
    } else {
        expected.duration_since(file_mtime).unwrap()
    };
    let diff_dir = if dir_mtime > expected {
        dir_mtime.duration_since(expected).unwrap()
    } else {
        expected.duration_since(dir_mtime).unwrap()
    };

    assert!(
        diff_file < Duration::from_secs(2),
        "File timestamp diff too large: {diff_file:?}"
    );
    assert!(
        diff_dir < Duration::from_secs(2),
        "Dir timestamp diff too large: {diff_dir:?}"
    );
}

#[tokio::test]
async fn test_tar_timestamp_preservation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let archive_path = temp_dir.path().join("test.tar.gz");
    let dest_dir = temp_dir.path().join("extracted_tar");

    let mtime = 1_735_732_800; // 2025-01-01 12:00:00 UTC
    let expected = SystemTime::UNIX_EPOCH + Duration::from_secs(mtime);

    {
        let file = File::create(&archive_path).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);

        // Add a directory with a specific timestamp
        let mut dir_header = tar::Header::new_gnu();
        dir_header.set_entry_type(tar::EntryType::Directory);
        dir_header.set_mode(0o755);
        dir_header.set_size(0);
        dir_header.set_mtime(mtime);
        dir_header.set_cksum();
        tar.append_data(&mut dir_header, "dir", &[] as &[u8])
            .unwrap();

        // Add a file with a specific timestamp
        let mut file_header = tar::Header::new_gnu();
        file_header.set_mode(0o644);
        file_header.set_size(7);
        file_header.set_mtime(mtime);
        file_header.set_cksum();
        tar.append_data(&mut file_header, "dir/file.txt", b"content" as &[u8])
            .unwrap();

        tar.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let local_fs = LocalFs::new();
    let (raw_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let progress = TaskProgressContext {
        id: 1,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    };

    let result: anyhow::Result<()> = archive_fs
        .extract(Path::new("."), &local_fs, &dest_dir, &progress)
        .await
        .unwrap();
    result.unwrap();

    // Verify file timestamp
    let file_meta = std::fs::metadata(dest_dir.join("dir/file.txt")).unwrap();
    let file_mtime = file_meta.modified().unwrap();

    // Verify directory timestamp
    let dir_meta = std::fs::metadata(dest_dir.join("dir")).unwrap();
    let dir_mtime = dir_meta.modified().unwrap();

    let diff_file = if file_mtime > expected {
        file_mtime.duration_since(expected).unwrap()
    } else {
        expected.duration_since(file_mtime).unwrap()
    };
    let diff_dir = if dir_mtime > expected {
        dir_mtime.duration_since(expected).unwrap()
    } else {
        expected.duration_since(dir_mtime).unwrap()
    };

    assert!(
        diff_file < Duration::from_secs(1),
        "File timestamp diff too large: {diff_file:?}"
    );
    assert!(
        diff_dir < Duration::from_secs(1),
        "Dir timestamp diff too large: {diff_dir:?}"
    );
}
