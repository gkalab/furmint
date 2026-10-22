use super::*;

#[tokio::test]
async fn test_handle_copy_move_into_zip_root() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("file.txt");
    std::fs::write(&src_file, b"hello").unwrap();

    let archive_path = temp_dir.path().join("add.zip");
    let local = LocalFs::new();
    local.create_file(&archive_path).await.unwrap();
    let archive = Arc::new(crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap());

    // The archive panel shows the archive root, so that is what the dialog prefills.
    let mut app = archive_dest_app(&src_dir, "file.txt", false, archive.clone());
    crate::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);
    assert_eq!(app.popups.copy_move.destination_input.text(), "/");

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(
        wait_for_archive_entry(&archive, "file.txt").await,
        "file.txt was not added to the zip archive root"
    );
}

#[tokio::test]
async fn test_handle_copy_move_directory_into_zip_root() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    let src_tree = src_dir.join("tree");
    std::fs::create_dir_all(src_tree.join("inner")).unwrap();
    std::fs::write(src_tree.join("top.txt"), b"top").unwrap();
    std::fs::write(src_tree.join("inner/deep.txt"), b"deep").unwrap();

    let archive_path = temp_dir.path().join("add_dir.zip");
    let local = LocalFs::new();
    local.create_file(&archive_path).await.unwrap();
    let archive = Arc::new(crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap());

    let mut app = archive_dest_app(&src_dir, "tree", true, archive.clone());
    crate::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);
    assert_eq!(app.popups.copy_move.destination_input.text(), "/");

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(
        wait_for_archive_entry(&archive, "tree/top.txt").await,
        "tree/top.txt was not added to the zip archive root"
    );
    assert!(
        wait_for_archive_entry(&archive, "tree/inner/deep.txt").await,
        "tree/inner/deep.txt was not added to the zip archive root"
    );
}

#[tokio::test]
async fn test_handle_copy_move_into_7z_root() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("file.txt");
    std::fs::write(&src_file, b"hello").unwrap();

    let archive_path = temp_dir.path().join("add.7z");
    let local = LocalFs::new();
    local.create_file(&archive_path).await.unwrap();
    let archive = Arc::new(crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap());

    let mut app = archive_dest_app(&src_dir, "file.txt", false, archive.clone());
    crate::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(
        wait_for_archive_entry(&archive, "file.txt").await,
        "file.txt was not added to the 7z archive root"
    );
}

#[tokio::test]
async fn test_handle_copy_move_into_zip_subdir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("file.txt");
    std::fs::write(&src_file, b"hello").unwrap();

    let archive_path = temp_dir.path().join("subdir.zip");
    let local = LocalFs::new();
    local.create_file(&archive_path).await.unwrap();
    let archive = Arc::new(crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap());
    archive
        .create_dir(std::path::Path::new("folder"))
        .await
        .unwrap();

    let mut app = archive_dest_app(&src_dir, "file.txt", false, archive.clone());
    // Navigate the archive panel into a subdirectory, then copy there.
    app.panels.right.active_tab_mut().current_dir = std::path::PathBuf::from("/folder");
    crate::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);
    assert_eq!(app.popups.copy_move.destination_input.text(), "/folder");

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(
        wait_for_archive_entry(&archive, "folder/file.txt").await,
        "file.txt was not added to the archive subdirectory"
    );
}

#[tokio::test]
async fn test_handle_paste_into_zip_root() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("file.txt");
    std::fs::write(&src_file, b"hello").unwrap();

    let archive_path = temp_dir.path().join("paste.zip");
    let local = LocalFs::new();
    local.create_file(&archive_path).await.unwrap();
    let archive = Arc::new(crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap());

    let mut app = archive_dest_app(&src_dir, "file.txt", false, archive.clone());
    // Paste into the archive panel.
    app.panels.active = PanelSide::Right;

    let data = FileClipboardData {
        action: FileClipboardAction::Copy,
        paths: vec![src_file],
        source_provider: Arc::new(LocalFs::new()),
    };
    app.os.clipboard.set(data).unwrap();

    handle_paste(&mut app).await;

    assert!(
        wait_for_archive_entry(&archive, "file.txt").await,
        "file.txt was not pasted into the zip archive root"
    );
}

#[tokio::test]
async fn test_handle_paste_directory_into_7z_root() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    let src_tree = src_dir.join("tree");
    std::fs::create_dir_all(src_tree.join("inner")).unwrap();
    std::fs::write(src_tree.join("top.txt"), b"top").unwrap();
    std::fs::write(src_tree.join("inner/deep.txt"), b"deep").unwrap();

    let archive_path = temp_dir.path().join("paste.7z");
    let local = LocalFs::new();
    local.create_file(&archive_path).await.unwrap();
    let archive = Arc::new(crate::fs::archive_fs::ArchiveFs::new(&archive_path).unwrap());

    let mut app = archive_dest_app(&src_dir, "tree", true, archive.clone());
    app.panels.active = PanelSide::Right;

    let data = FileClipboardData {
        action: FileClipboardAction::Copy,
        paths: vec![src_tree],
        source_provider: Arc::new(LocalFs::new()),
    };
    app.os.clipboard.set(data).unwrap();

    handle_paste(&mut app).await;

    assert!(
        wait_for_archive_entry(&archive, "tree/top.txt").await,
        "tree/top.txt was not pasted into the 7z archive root"
    );
    assert!(
        wait_for_archive_entry(&archive, "tree/inner/deep.txt").await,
        "tree/inner/deep.txt was not pasted into the 7z archive root"
    );
}

#[tokio::test]
async fn test_handle_copy_move_between_two_archives() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_zip = temp_dir.path().join("src.zip");
    let dest_zip = temp_dir.path().join("dest.zip");
    {
        let file = std::fs::File::create(&src_zip).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("tree/top.txt", options).unwrap();
        zip.write_all(b"top").unwrap();
        zip.start_file("tree/sub/deep.txt", options).unwrap();
        zip.write_all(b"deep").unwrap();
        zip.finish().unwrap();
    }
    let local = LocalFs::new();
    local.create_file(&dest_zip).await.unwrap();

    let src_archive = Arc::new(crate::fs::archive_fs::ArchiveFs::new(&src_zip).unwrap());
    let dest_archive = Arc::new(crate::fs::archive_fs::ArchiveFs::new(&dest_zip).unwrap());

    // Active panel: the source archive, with "tree" selected.
    let mut app = minimal_state_with_entries(
        PanelSide::Left,
        vec![make_fileentry("tree", true, true)],
        vec![],
        0,
        0,
    );
    app.panels.left.active_tab_mut().provider = src_archive;
    app.panels.left.active_tab_mut().current_dir = PathBuf::from("/");

    // Inactive panel: the destination archive at its root.
    let mut dest_tab = make_tab("/", vec![], 0);
    dest_tab.provider = dest_archive.clone();
    app.panels.right = make_tab_manager(dest_tab);

    crate::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);
    assert_eq!(app.popups.copy_move.destination_input.text(), "/");

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(
        wait_for_archive_entry(&dest_archive, "tree/top.txt").await,
        "tree/top.txt was not copied between the archives"
    );
    assert!(
        wait_for_archive_entry(&dest_archive, "tree/sub/deep.txt").await,
        "tree/sub/deep.txt was not copied between the archives"
    );
    assert_eq!(
        dest_archive
            .read_file(std::path::Path::new("tree/sub/deep.txt"))
            .await
            .unwrap(),
        b"deep"
    );
}

/// Poll until the archive contains `path` (the copy task runs on a spawned task).
async fn wait_for_archive_entry(archive: &crate::fs::archive_fs::ArchiveFs, path: &str) -> bool {
    let start = std::time::Instant::now();
    while !archive.exists(std::path::Path::new(path)).await {
        if start.elapsed() > std::time::Duration::from_secs(5) {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    true
}

/// Builds an app whose *inactive* panel shows `archive`, and whose active panel is
/// a local directory `src_dir` containing the selected `selected` entry.
fn archive_dest_app(
    src_dir: &std::path::Path,
    selected: &str,
    selected_is_dir: bool,
    archive: Arc<crate::fs::archive_fs::ArchiveFs>,
) -> AppState {
    let mut app = minimal_state_with_entries(
        PanelSide::Left,
        vec![make_fileentry(selected, true, selected_is_dir)],
        vec![],
        0,
        0,
    );
    app.panels.left.active_tab_mut().current_dir = src_dir.to_path_buf();

    let mut archive_tab = make_tab("/", vec![], 0);
    archive_tab.provider = archive;
    app.panels.right = make_tab_manager(archive_tab);
    app
}
