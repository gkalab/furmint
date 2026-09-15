use async_trait::async_trait;
use fm::app::AppState;
use fm::app_state::tabs::Tab;
use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::{ContextKey, FileMetadata, FileSystemProvider};
use fm::fs::utils::FileEntry;
use fm::handlers::popup_rename::{handle_init_rename, handle_rename_event};
use fm::tasks::UiEvent;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use termina::event::{KeyCode, Modifiers};
use tokio::sync::mpsc;

async fn test_app_with_entry(name: &str, is_dir: bool, path: &std::path::Path) -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<UiEvent>();

    let mut tab = Tab::new(path).await.unwrap();
    tab.entries.clear();
    tab.entries.push(FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: Some(123),
        modified: None,
        attributes: String::from("-rw-r--r--"),
        selected: false,
    });
    tab.cursor = 0;
    let mut left_tm = fm::app_state::tabs::TabManager::new(path).await.unwrap();
    left_tm.tabs[0] = tab;
    fm::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(fm::app_state::tabs::TabManager::new(path).await.unwrap())
        .task_tx(task_tx)
        .build()
}

#[tokio::test]
async fn test_init_rename_for_normal_file() {
    let mut app = test_app_with_entry("myfile.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    assert_eq!(app.popups.rename.original_name, "myfile.txt");
    // Cursor should be placed before extension
    assert!(app.popups.rename.cursor_position < app.popups.rename.original_name.len());
}

#[tokio::test]
async fn test_init_rename_skips_dotdot() {
    let mut app = test_app_with_entry("..", true, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    assert!(!app.popups.rename.is_visible);
}

#[tokio::test]
async fn test_rename_typing_and_backspace() {
    let mut app = test_app_with_entry("file.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    let orig = app.popups.rename.new_name.clone();
    handle_rename_event(KeyCode::Char('a'), Modifiers::NONE, &mut app).await;
    assert_ne!(app.popups.rename.new_name, orig);
    handle_rename_event(KeyCode::Backspace, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.rename.new_name, orig);
}

#[tokio::test]
async fn test_rename_esc_resets() {
    let mut app = test_app_with_entry("other.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    handle_rename_event(KeyCode::Escape, Modifiers::NONE, &mut app).await;
    assert!(!app.popups.rename.is_visible);
}

#[tokio::test]
async fn test_rename_enter_same_name_resets() {
    let mut app = test_app_with_entry("foo.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    assert!(app.popups.rename.is_visible);
    handle_rename_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(!app.popups.rename.is_visible);
}

#[tokio::test]
async fn test_rename_navigation() {
    let mut app = test_app_with_entry("test.txt", false, &std::env::temp_dir()).await;
    handle_init_rename(&mut app);
    // Original name is test.txt, stem is test (len 4), cursor should be at 4
    assert_eq!(app.popups.rename.cursor_position, 4);

    handle_rename_event(KeyCode::Home, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.rename.cursor_position, 0);

    handle_rename_event(KeyCode::End, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.rename.cursor_position, 8); // test.txt len

    handle_rename_event(KeyCode::Left, Modifiers::NONE, &mut app).await;
    assert_eq!(app.popups.rename.cursor_position, 7);

    handle_rename_event(KeyCode::Delete, Modifiers::NONE, &mut app).await; // delete last 't'
    assert_eq!(app.popups.rename.new_name, "test.tx");
}

// A provider
// without replace semantics (SFTP/OpenSSH: SSH_FXP_RENAME has no overwrite
// flag) rejects the rename, so the user confirms "Overwrite?" and still gets
// "Error renaming: ...". This wrapper emulates exactly that.
struct SftpLikeFs {
    inner: LocalFs,
}

#[async_trait]
impl FileSystemProvider for SftpLikeFs {
    async fn rename(&self, from: &Path, to: &Path) -> anyhow::Result<()> {
        if self.inner.exists(to).await {
            return Err(anyhow::anyhow!(
                "No such file or directory: {}",
                to.display()
            ));
        }
        self.inner.rename(from, to).await
    }

    async fn list_dir(&self, path: &Path) -> anyhow::Result<Vec<FileEntry>> {
        self.inner.list_dir(path).await
    }
    async fn create_dir(&self, path: &Path) -> anyhow::Result<()> {
        self.inner.create_dir(path).await
    }
    async fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        self.inner.create_dir_all(path).await
    }
    async fn create_file(&self, path: &Path) -> anyhow::Result<()> {
        self.inner.create_file(path).await
    }
    async fn delete(&self, path: &Path, recursive: bool) -> anyhow::Result<()> {
        self.inner.delete(path, recursive).await
    }
    async fn read_file(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        self.inner.read_file(path).await
    }
    async fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> anyhow::Result<Vec<u8>> {
        self.inner.read_file_at(path, offset, len).await
    }
    async fn write_file(&self, path: &Path, data: &[u8]) -> anyhow::Result<()> {
        self.inner.write_file(path, data).await
    }
    async fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> anyhow::Result<()> {
        self.inner.write_file_at(path, offset, data).await
    }
    fn display_prefix(&self) -> &'static str {
        "local"
    }
    fn is_local(&self) -> bool {
        true
    }
    async fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path).await
    }
    async fn is_dir(&self, path: &Path) -> bool {
        self.inner.is_dir(path).await
    }
    async fn canonicalize(&self, path: &Path) -> anyhow::Result<PathBuf> {
        self.inner.canonicalize(path).await
    }
    async fn get_file_info(&self, path: &Path) -> Option<FileMetadata> {
        self.inner.get_file_info(path).await
    }
    async fn get_permissions(&self, path: &Path) -> Option<u32> {
        self.inner.get_permissions(path).await
    }
    async fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        self.inner.set_permissions(path, mode).await
    }
    async fn get_modified_time(&self, path: &Path) -> Option<SystemTime> {
        self.inner.get_modified_time(path).await
    }
    async fn set_modified_time(&self, path: &Path, mtime: SystemTime) -> bool {
        self.inner.set_modified_time(path, mtime).await
    }
    fn context_key(&self) -> ContextKey {
        ContextKey::Local
    }
    fn display_path(&self, path: &Path) -> String {
        self.inner.display_path(path)
    }
    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64> {
        self.inner.calc_dir_size(path).await
    }
}

#[tokio::test]
async fn test_rename_confirmed_overwrite_replaces_existing() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let file1 = dir.join("file1.txt");
    let file2 = dir.join("file2.txt");
    std::fs::write(&file1, b"content-1").unwrap();
    std::fs::write(&file2, b"content-2").unwrap();

    let (task_tx, _task_rx) = mpsc::unbounded_channel::<UiEvent>();
    let mut tab = Tab::new(dir).await.unwrap();
    tab.entries.clear();
    tab.entries.push(FileEntry {
        name: "file1.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(9),
        modified: None,
        attributes: "-rw-r--r--".to_string(),
        selected: false,
    });
    tab.cursor = 0;
    tab.provider = Arc::new(SftpLikeFs {
        inner: LocalFs::new(),
    });

    let mut left_tm = fm::app_state::tabs::TabManager::new(dir).await.unwrap();
    left_tm.tabs[0] = tab;
    let mut app = fm::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(fm::app_state::tabs::TabManager::new(dir).await.unwrap())
        .task_tx(task_tx)
        .build();

    handle_init_rename(&mut app);
    app.popups.rename.new_name = "file2.txt".to_string();
    app.popups.rename.cursor_position = 9;

    handle_rename_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(app.popups.rename.show_overwrite_confirm);

    // User confirms "Overwrite?" -> the rename must replace file2.txt.
    handle_rename_event(KeyCode::Char('y'), Modifiers::NONE, &mut app).await;

    let err = app.active_tab_mut().error.clone();
    assert!(
        err.is_none(),
        "rename failed although the user confirmed Overwrite: {err:?}"
    );
    assert!(!file1.exists(), "source file was not moved");
    assert_eq!(std::fs::read(&file2).unwrap(), b"content-1");
}

#[tokio::test]
async fn test_rename_overwrite_flow() {
    use std::fs::File;
    // Use tempfile for guaranteed isolation and cleanup
    let temp = tempfile::tempdir().unwrap();
    let temp_dir = temp.path();

    let file1 = temp_dir.join("file1.txt");
    let file2 = temp_dir.join("file2.txt");
    File::create(&file1).unwrap();
    File::create(&file2).unwrap();

    let mut app = test_app_with_entry("file1.txt", false, temp_dir).await;
    handle_init_rename(&mut app);

    // Rename file1 to file2
    app.popups.rename.new_name = "file2.txt".to_string();
    app.popups.rename.cursor_position = 9;

    handle_rename_event(KeyCode::Enter, Modifiers::NONE, &mut app).await;
    assert!(app.popups.rename.show_overwrite_confirm);

    handle_rename_event(KeyCode::Char('y'), Modifiers::NONE, &mut app).await;
    // On Unix, rename is usually successful.
    // We check if the popup was reset, which happens on success.
    assert!(!app.popups.rename.is_visible);
}
