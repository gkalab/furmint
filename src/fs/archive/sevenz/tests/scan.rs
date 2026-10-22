use super::*;

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
    let entries = archive_fs.list_dir(Path::new(".")).await.unwrap();

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"a.txt"),
        "root should contain a.txt, got {names:?}"
    );
    assert!(
        names.contains(&"dir1"),
        "root should contain dir1, got {names:?}"
    );

    let sub_entries = archive_fs.list_dir(Path::new("dir1")).await.unwrap();
    let sub_names: Vec<&str> = sub_entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        sub_names.contains(&"b.txt"),
        "dir1 should contain b.txt, got {sub_names:?}"
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
    assert!(!entry.is_dir);
    assert_eq!(entry.size, Some(4));
    assert!(entry.modified.is_some());
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
            .attributes,
        "-rwxr-xr-x"
    );
    assert_eq!(
        archive_fs
            .get_entry(Path::new("plain.txt"))
            .unwrap()
            .attributes,
        "-rw-r--r--"
    );
    assert_eq!(
        archive_fs
            .get_entry(Path::new("scripts"))
            .unwrap()
            .attributes,
        "drwxr-xr-x"
    );
}
