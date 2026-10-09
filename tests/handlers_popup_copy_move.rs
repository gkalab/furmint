use fm::app::AppState;
use fm::app_state::tabs::{PanelSide, Tab, TabManager};
use fm::clipboard::{FileClipboardAction, FileClipboardData};
use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::FileSystemProvider;
use fm::fs::utils::FileEntry;
use fm::handlers::popup_copy_move::{
    handle_copy_move_event, handle_init_copy, handle_init_move, handle_paste,
};
use fm::state::CopyMoveAction;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use termina::event::{KeyCode, Modifiers};

fn make_fileentry(name: &str, selected: bool, is_dir: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected,
    }
}

fn make_tab(path: &str, entries: Vec<FileEntry>, cursor: usize) -> Tab {
    let current_dir = PathBuf::from(path);
    Tab {
        provider: Arc::new(LocalFs::new()),
        current_dir: current_dir.clone(),
        entries,
        cursor,
        search: fm::app_state::tabs::IncrementalSearch::default(),
        sort: fm::app_state::tabs::SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        ssh_session_id: None,
        status_msg: None,
        dir_sizes: HashMap::new(),
        is_reloading: false,
        visible_indices: Vec::new(),
        filter: fm::state::FileFilterState::new(),
    }
}

fn make_tab_manager(tab: Tab) -> TabManager {
    TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    }
}

fn minimal_state_with_entries(
    active: PanelSide,
    left_entries: Vec<FileEntry>,
    right_entries: Vec<FileEntry>,
    left_cursor: usize,
    right_cursor: usize,
) -> AppState {
    let mut app = fm::test_utils::TestAppBuilder::new()
        .left(make_tab_manager(make_tab(
            "/left",
            left_entries,
            left_cursor,
        )))
        .right(make_tab_manager(make_tab(
            "/right",
            right_entries,
            right_cursor,
        )))
        .build();
    app.panels.active = active;
    app
}

#[tokio::test]
async fn test_init_copy_and_move_selects_correct_paths() {
    // Left panel active, selected entry (not '..'), should populate paths
    let left_entries = vec![
        make_fileentry("A.txt", true, false),
        make_fileentry("..", false, true),
    ];
    let right_entries = vec![make_fileentry("X", false, false)];
    let mut app =
        minimal_state_with_entries(PanelSide::Left, left_entries, right_entries.clone(), 0, 0);
    handle_init_copy(&mut app);
    assert!(app.popups.copy_move.is_visible);
    assert_eq!(app.popups.copy_move.action, CopyMoveAction::Copy);
    assert!(app.popups.copy_move.source_paths[0].ends_with("A.txt"));

    let left_entries = vec![
        make_fileentry("B.txt", false, false),
        make_fileentry("..", false, true),
    ];
    let mut app =
        minimal_state_with_entries(PanelSide::Left, left_entries, right_entries.clone(), 0, 0);
    handle_init_move(&mut app);
    assert_eq!(app.popups.copy_move.action, CopyMoveAction::Move);
}

#[tokio::test]
async fn test_init_copy_for_no_selection_uses_current_if_not_parent() {
    let left_entries = vec![
        make_fileentry("foo", false, false),
        make_fileentry("..", false, true),
    ];
    // Cursor points to "foo"
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    assert!(app.popups.copy_move.source_paths[0].ends_with("foo"));
}

#[tokio::test]
async fn test_init_copy_for_parent_dir_does_nothing() {
    let left_entries = vec![make_fileentry("..", false, true)];
    // Cursor points to ".."
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    assert!(!app.popups.copy_move.is_visible);
    assert_eq!(app.popups.copy_move.source_paths.len(), 0);
}

#[tokio::test]
async fn test_handle_copy_move_event_char_and_edit() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.destination_input.clear();
    app.popups.copy_move.cursor_position = 0;
    // Insert 'x'
    handle_copy_move_event(KeyCode::Char('x'), Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input, "x");
    assert_eq!(app.popups.copy_move.cursor_position, 1);
    // Insert 'y' at position 1
    handle_copy_move_event(KeyCode::Char('y'), Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input, "xy");
    assert_eq!(app.popups.copy_move.cursor_position, 2);
    // Backspace
    handle_copy_move_event(KeyCode::Backspace, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input, "x");
    assert_eq!(app.popups.copy_move.cursor_position, 1);
    // Left
    handle_copy_move_event(KeyCode::Left, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.cursor_position, 0);
    // Delete (removes 'x')
    handle_copy_move_event(KeyCode::Delete, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.destination_input, "");
    assert_eq!(app.popups.copy_move.cursor_position, 0);
}

#[tokio::test]
async fn test_handle_copy_move_event_navigation_keys() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.destination_input = "abcdef".to_string();
    app.popups.copy_move.cursor_position = 3;
    // Home
    handle_copy_move_event(KeyCode::Home, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.cursor_position, 0);
    // End
    handle_copy_move_event(KeyCode::End, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.cursor_position, 6);
    // Right at end (should stay)
    handle_copy_move_event(KeyCode::Right, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.copy_move.cursor_position, 6);
}

#[tokio::test]
async fn test_handle_copy_move_event_escape_resets_popup() {
    let left_entries = vec![make_fileentry("a", true, false)];
    let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
    handle_init_copy(&mut app);
    app.popups.copy_move.error = Some("some error".to_string());
    assert!(app.popups.copy_move.is_visible);
    handle_copy_move_event(KeyCode::Escape, Modifiers::NONE, &mut app).await;
    assert!(!app.popups.copy_move.is_visible);
    assert!(app.popups.copy_move.error.is_none());
}

#[tokio::test]
async fn test_handle_copy_move_event_home_dir_expansion() {
    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups
        .set_popup_visible(fm::app::PopupKind::CopyMove, true);
    app.popups.copy_move.destination_input = "~".to_string();

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    if let Some(base_dirs) = directories::BaseDirs::new() {
        let home = base_dirs
            .home_dir()
            .canonicalize()
            .unwrap_or_else(|_| base_dirs.home_dir().to_path_buf());
        let _home_str = home.to_string_lossy().to_string();
        // It resets on success, but we can check if it's not visible anymore
        assert!(!app.popups.copy_move.is_visible);
    }
}

#[tokio::test]
async fn test_handle_copy_move_validation_same_path() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let file_path = temp_dir.join("test_file_copy_val.txt");
    std::fs::File::create(&file_path).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups
        .set_popup_visible(fm::app::PopupKind::CopyMove, true);
    app.popups.copy_move.source_paths = vec![file_path.clone()];
    app.popups.copy_move.destination_input = temp_dir.to_string_lossy().to_string();

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(app.popups.copy_move.error.is_some());
    assert!(
        app.popups
            .copy_move
            .error
            .as_ref()
            .unwrap()
            .contains("same")
    );

    std::fs::remove_file(&file_path).ok();
}

#[tokio::test]
async fn test_handle_copy_move_validation_into_itself() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let src_dir = temp_dir.join("test_src_dir");
    std::fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = src_dir.join("test_dest_dir");

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.popups
        .set_popup_visible(fm::app::PopupKind::CopyMove, true);
    app.popups.copy_move.source_paths = vec![src_dir.clone()];
    app.popups.copy_move.destination_input = dest_dir.to_string_lossy().to_string();

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(app.popups.copy_move.error.is_some());
    assert!(
        app.popups
            .copy_move
            .error
            .as_ref()
            .unwrap()
            .contains("subdirectory of itself")
    );

    std::fs::remove_dir_all(&src_dir).ok();
}

#[tokio::test]
async fn test_handle_paste_validation_same_path() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let file_path = temp_dir.join("test_file_paste_val.txt");
    std::fs::File::create(&file_path).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.panels.left.active_tab_mut().current_dir = temp_dir.clone();

    let data = FileClipboardData {
        action: FileClipboardAction::Copy,
        paths: vec![file_path.clone()],
        source_provider: Arc::new(LocalFs::new()),
    };
    app.os.clipboard.set(data).unwrap();

    handle_paste(&mut app).await;

    assert!(app.panels.left.active_tab().error.is_some());
    assert!(
        app.panels
            .left
            .active_tab()
            .error
            .as_ref()
            .unwrap()
            .contains("same")
    );

    std::fs::remove_file(&file_path).ok();
}

/// Poll for a file to appear (the copy task runs on a spawned tokio task).
async fn wait_for_file(path: &std::path::Path, deadline_secs: u64) -> bool {
    let start = std::time::Instant::now();
    while !path.exists() {
        if start.elapsed() > std::time::Duration::from_secs(deadline_secs) {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    true
}

#[tokio::test]
async fn test_handle_copy_move_relative_parent_dir_resolves_against_active_tab() {
    let root = std::env::temp_dir().join("fm_rel_parent_test");
    let _ = std::fs::remove_dir_all(&root);
    let src_dir = root.join("a");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("file.txt");
    std::fs::File::create(&src_file).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.panels.left.active_tab_mut().current_dir = src_dir;
    app.popups
        .set_popup_visible(fm::app::PopupKind::CopyMove, true);
    app.popups.copy_move.source_paths = vec![src_file];
    app.popups.copy_move.destination_input = "..".to_string();

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    // ".." must resolve against the active tab's current dir, not the process cwd
    let expected = root.join("file.txt");
    assert!(
        wait_for_file(&expected, 5).await,
        "file was not copied to the parent of the active tab's current directory"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn test_handle_copy_move_relative_subdir_resolves_against_active_tab() {
    let root = std::env::temp_dir().join("fm_rel_subdir_test");
    let _ = std::fs::remove_dir_all(&root);
    let other_dir = root.join("other");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::create_dir_all(&other_dir).unwrap();
    std::fs::File::create(other_dir.join("a.txt")).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    app.panels.left.active_tab_mut().current_dir = other_dir.clone();
    app.popups
        .set_popup_visible(fm::app::PopupKind::CopyMove, true);
    app.popups.copy_move.source_paths = vec![other_dir.join("a.txt")];
    app.popups.copy_move.destination_input = "../target".to_string();

    handle_copy_move_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;

    assert!(
        wait_for_file(&target_dir.join("a.txt"), 5).await,
        "file was not copied to ../target relative to the active tab's current directory"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// Poll until the archive contains `path` (the copy task runs on a spawned task).
async fn wait_for_archive_entry(archive: &fm::fs::fs_archive::ArchiveFs, path: &str) -> bool {
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
    archive: Arc<fm::fs::fs_archive::ArchiveFs>,
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
    let archive = Arc::new(fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap());

    // The archive panel shows the archive root, so that is what the dialog prefills.
    let mut app = archive_dest_app(&src_dir, "file.txt", false, archive.clone());
    fm::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);
    assert_eq!(app.popups.copy_move.destination_input, "/");

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
    let archive = Arc::new(fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap());

    let mut app = archive_dest_app(&src_dir, "tree", true, archive.clone());
    fm::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);
    assert_eq!(app.popups.copy_move.destination_input, "/");

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
    let archive = Arc::new(fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap());

    let mut app = archive_dest_app(&src_dir, "file.txt", false, archive.clone());
    fm::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);

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
    let archive = Arc::new(fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap());
    archive
        .create_dir(std::path::Path::new("folder"))
        .await
        .unwrap();

    let mut app = archive_dest_app(&src_dir, "file.txt", false, archive.clone());
    // Navigate the archive panel into a subdirectory, then copy there.
    app.panels.right.active_tab_mut().current_dir = std::path::PathBuf::from("/folder");
    fm::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);
    assert_eq!(app.popups.copy_move.destination_input, "/folder");

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
    let archive = Arc::new(fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap());

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
    let archive = Arc::new(fm::fs::fs_archive::ArchiveFs::new(&archive_path).unwrap());

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

    let src_archive = Arc::new(fm::fs::fs_archive::ArchiveFs::new(&src_zip).unwrap());
    let dest_archive = Arc::new(fm::fs::fs_archive::ArchiveFs::new(&dest_zip).unwrap());

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

    fm::handlers::popup_copy_move::init_copy_move(&mut app, CopyMoveAction::Copy);
    assert_eq!(app.popups.copy_move.destination_input, "/");

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

#[tokio::test]
async fn test_handle_paste_clears_clipboard_on_move() {
    let temp_dir = std::env::temp_dir().canonicalize().unwrap();
    let src_file = temp_dir.join("test_paste_clear_src.txt");
    std::fs::File::create(&src_file).unwrap();

    let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
    // Navigate to different dir to pass path validation
    let dest_dir = temp_dir.join("test_paste_clear_dest");
    std::fs::create_dir_all(&dest_dir).unwrap();
    app.panels.left.active_tab_mut().current_dir = dest_dir.clone();

    let data = FileClipboardData {
        action: FileClipboardAction::Cut,
        paths: vec![src_file.clone()],
        source_provider: Arc::new(LocalFs::new()),
    };
    app.os.clipboard.set(data).unwrap();

    handle_paste(&mut app).await;

    // Clipboard should be empty now
    assert!(app.os.clipboard.get().unwrap().is_none());

    std::fs::remove_file(&src_file).ok();
    std::fs::remove_dir_all(&dest_dir).ok();
}

#[tokio::test]
async fn test_handle_clipboard_action_sets_message() {
    let entries = vec![FileEntry {
        name: "test.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(10),
        modified: None,
        attributes: String::new(),
        selected: true,
    }];
    let mut app = minimal_state_with_entries(PanelSide::Left, entries, vec![], 0, 0);

    fm::handlers::popup_copy_move::handle_clipboard_copy(&mut app);

    assert!(app.active_tab().status_msg.is_some());
    let (msg, _) = app.active_tab().status_msg.as_ref().unwrap();
    assert_eq!(msg, "1 item copied");
}
