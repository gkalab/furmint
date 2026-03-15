use fm::fs::fs_archive::ArchiveFs;
use fm::fs::fs_provider::FileSystemProvider;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;
use zip::DateTime;
use zip::write::SimpleFileOptions;

#[tokio::test]
async fn test_zip_timestamps() {
    let temp_dir = tempfile::tempdir().unwrap();
    let zip_path = temp_dir.path().join("test.zip");

    {
        let file = File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = SimpleFileOptions::default()
            .last_modified_time(DateTime::from_date_and_time(2025, 1, 1, 12, 0, 0).unwrap());

        zip.start_file("test.txt", options).unwrap();
        zip.write_all(b"hello").unwrap();
        zip.finish().unwrap();
    }

    let archive_fs = ArchiveFs::new(&zip_path).unwrap();
    let entries = archive_fs.list_dir(Path::new(".")).unwrap();

    let entry = entries
        .iter()
        .find(|e| e.name == "test.txt")
        .expect("test.txt not found");

    assert!(
        entry.modified.is_some(),
        "Modified time should be present for test.txt"
    );
    let modified = entry.modified.unwrap();

    // Check if it's not the Unix epoch (1970-01-01)
    let epoch = SystemTime::UNIX_EPOCH;
    assert!(
        modified > epoch,
        "Modified time should be after Unix epoch, but got {modified:?}"
    );
}
