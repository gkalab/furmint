use super::*;

#[tokio::test]
async fn test_update_viewer_content_loads_large_file() {
    use std::io::Write;
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("large.txt");
    let mut f = std::fs::File::create(&file_path).unwrap();
    let mb = 11;
    // Create a file with distinct lines to verify indexing
    for i in 0..1000 {
        writeln!(f, "Line {i:04}").unwrap();
    }
    // Fill the rest to reach 11MB
    f.write_all(&vec![b'a'; mb * 1024 * 1024]).unwrap();

    let mut app = test_app(vec![FileEntry {
        name: "large.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(std::fs::metadata(&file_path).unwrap().len()),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.file_viewer.is_visible = true;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<crate::state::ContentLoadResult>();
    app.file_viewer.text.set_content_load_channel(tx);
    update_viewer_content(&mut app).await;

    // Indexing runs in a background task; collect its result
    let res = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
        .await
        .expect("timed out waiting for large file index")
        .expect("channel closed");
    app.file_viewer.handle_content_load_result(res);

    // Should NOT have error message in content
    assert_eq!(app.file_viewer.text.content, [] as [std::string::String; 0]);
    // Should have large file components initialized
    assert!(app.file_viewer.text.large_file_reader.is_some());
    assert!(app.file_viewer.text.large_file_indexer.is_some());

    // Verify we can read the first line
    let indexer = app.file_viewer.text.large_file_indexer.as_ref().unwrap();
    let reader = app.file_viewer.text.large_file_reader.as_ref().unwrap();
    let (s, e) = indexer.get_line_with_reader(0, reader).unwrap();
    assert_eq!(reader.get_chunk(s, e).trim(), "Line 0000");
}

#[tokio::test]
async fn test_update_viewer_content_describes_binary() {
    use std::io::Write;
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("bin.dat");
    let mut f = std::fs::File::create(&file_path).unwrap();
    let mut data = vec![0u8; 9000];
    data[10] = 0; // null byte early in file
    f.write_all(&data).unwrap();
    let mut app = test_app(vec![FileEntry {
        name: "bin.dat".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(9000),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.file_viewer.is_visible = true;
    update_viewer_content(&mut app).await;

    let content = &app.file_viewer.text.content;
    assert!(!content.is_empty(), "a binary must show something");
    let joined = content.join(" ");
    if joined == "Binary file detected" {
        // `file` is not installed: the viewer keeps its own message.
    } else {
        // The description comes from `file`, which reports this as "data".
        assert!(
            joined.to_lowercase().contains("data"),
            "unexpected `file` description: {joined}"
        );
    }
}

#[tokio::test]
async fn test_update_viewer_content_reads_text_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("hello.txt");
    std::fs::write(&file_path, "Hello, F3!").unwrap();
    let mut app = test_app(vec![FileEntry {
        name: "hello.txt".to_string(),

        is_dir: false,
        is_symlink: false,
        size: Some(11),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.file_viewer.is_visible = true;
    update_viewer_content(&mut app).await;
    assert_eq!(app.file_viewer.text.content[0], "Hello, F3!");
}

/// Selecting `..` is a navigation shortcut, so the viewer names the entry
/// instead of previewing (and listing) the parent directory.
#[tokio::test]
async fn test_update_viewer_content_shows_parent_directory_label() {
    let mut app = test_app(vec![FileEntry {
        name: "..".to_string(),
        is_dir: true,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    app.panels.left.active_tab_mut().current_dir = std::path::PathBuf::from("/tmp/sub");
    app.file_viewer.is_visible = true;
    update_viewer_content(&mut app).await;

    assert_eq!(
        app.file_viewer.text.content,
        vec!["Parent directory".to_string()],
        "`..` must render as a label, not a listing"
    );
    assert!(
        app.file_viewer.text.directory_rows.is_none(),
        "the parent directory's entries must not be listed"
    );
}

/// Opening an archive activates a new tab; a visible viewer must refresh to the
/// archive's selected entry instead of keeping the file selected before.
#[tokio::test]
async fn test_open_archive_updates_viewer_content() {
    use std::io::Write;
    let temp_dir = tempfile::tempdir().unwrap();
    let local_file = temp_dir.path().join("hello.txt");
    std::fs::write(&local_file, "Hello, F3!").unwrap();

    let archive_path = temp_dir.path().join("inner.zip");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("inner.txt", options).unwrap();
        zip.write_all(b"archive content").unwrap();
        zip.finish().unwrap();
    }

    let metadata = std::fs::metadata(&archive_path).unwrap();
    let provider =
        std::sync::Arc::new(crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap());

    let mut app = test_app(vec![FileEntry {
        name: "hello.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(11),
        modified: None,
        attributes: String::new(),
        selected: false,
    }]);
    app.panels.left.active_tab_mut().current_dir = temp_dir.path().to_path_buf();
    app.file_viewer.is_visible = true;

    // Seed the archive cache so `handle_open_archive` takes the sync path.
    app.cache.archive_cache.insert(
        archive_path.clone(),
        crate::app::ArchiveCacheEntry {
            mtime: metadata.modified().unwrap(),
            size: metadata.len(),
            provider,
            closed_at: None,
        },
    );

    handle_open_archive(&mut app, &archive_path, "inner.zip".to_string()).await;

    let active = app.active_tab();
    assert_eq!(active.custom_title.as_deref(), Some("inner.zip"));
    assert_eq!(
        app.file_viewer.text.content,
        ["archive content".to_string()],
        "the viewer must show the archive's selected file, not the previous tab's"
    );
}
