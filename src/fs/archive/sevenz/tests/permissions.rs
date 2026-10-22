#[cfg(unix)]
use super::*;

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

    let src_fs = LocalFs::new();
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
            .attributes,
        "-rwxr-xr-x"
    );
    assert_eq!(
        archive_fs
            .get_entry(Path::new("bin/plain.txt"))
            .unwrap()
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

    let src_fs = LocalFs::new();
    let archive_fs = ArchiveFs::new(&archive_path).unwrap();
    let progress = make_progress();

    archive_fs
        .copy_from_local(&src_fs, &src_dir, Path::new("bin"), &progress)
        .await
        .unwrap()
        .unwrap();

    let extract_dir = temp_dir.path().join("extracted");
    std::fs::create_dir(&extract_dir).unwrap();
    let dest_fs = LocalFs::new();

    let result = archive_fs
        .extract(Path::new("."), &dest_fs, &extract_dir, &progress)
        .await
        .unwrap();
    result.unwrap();

    let meta = std::fs::metadata(extract_dir.join("bin/run.sh")).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o755);
    assert_ne!(meta.permissions().mode() & 0o100, 0);
}
