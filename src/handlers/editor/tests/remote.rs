use super::*;
use crate::app::AppState;
use crate::fs::utils::FileEntry;
use std::sync::Arc;

struct MockFileSystem {
    write_count: std::sync::atomic::AtomicUsize,
    write_data: std::sync::Mutex<Option<Vec<u8>>>,
    write_error: std::sync::atomic::AtomicBool,
    permissions_result: std::sync::Mutex<Option<u32>>,
}

impl MockFileSystem {
    fn new() -> Self {
        Self {
            write_count: std::sync::atomic::AtomicUsize::new(0),
            write_data: std::sync::Mutex::new(None),
            write_error: std::sync::atomic::AtomicBool::new(false),
            permissions_result: std::sync::Mutex::new(Some(0o644)),
        }
    }
}

#[async_trait::async_trait]
impl crate::fs::provider::FileSystemProvider for MockFileSystem {
    fn is_local(&self) -> bool {
        false
    }

    fn display_prefix(&self) -> &'static str {
        "mock://"
    }

    async fn list_dir(&self, _path: &std::path::Path) -> anyhow::Result<Vec<FileEntry>> {
        Ok(vec![])
    }

    async fn create_dir(&self, _path: &std::path::Path) -> anyhow::Result<()> {
        Ok(())
    }

    async fn create_dir_all(&self, _path: &std::path::Path) -> anyhow::Result<()> {
        Ok(())
    }

    async fn create_file(&self, _path: &std::path::Path) -> anyhow::Result<()> {
        Ok(())
    }

    async fn delete(&self, _path: &std::path::Path, _recursive: bool) -> anyhow::Result<()> {
        Ok(())
    }

    async fn rename(&self, _from: &std::path::Path, _to: &std::path::Path) -> anyhow::Result<()> {
        Ok(())
    }

    async fn read_file(&self, _path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
        Ok(b"original content".to_vec())
    }

    async fn read_file_at(
        &self,
        _path: &std::path::Path,
        _offset: u64,
        _len: usize,
    ) -> anyhow::Result<Vec<u8>> {
        Ok(b"original content".to_vec())
    }

    async fn write_file(&self, _path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
        self.write_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.write_data.lock().unwrap() = Some(data.to_vec());
        if self.write_error.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Mock write error"));
        }
        Ok(())
    }

    async fn write_file_at(
        &self,
        _path: &std::path::Path,
        _offset: u64,
        _data: &[u8],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn write_file_with_permissions(
        &self,
        path: &std::path::Path,
        data: &[u8],
        _mode: Option<u32>,
    ) -> anyhow::Result<()> {
        self.write_file(path, data).await
    }

    async fn read_file_content(
        &self,
        path: &std::path::Path,
        limit: usize,
    ) -> anyhow::Result<String> {
        let buffer = self.read_file(path).await?;
        if buffer.len() > limit {
            return Ok(format!(
                "File too large to display (size: {}, limit: {})",
                crate::fs::utils::format_size(Some(buffer.len() as u64), false, false),
                crate::fs::utils::format_size(Some(limit as u64), false, false)
            ));
        }
        if buffer[..buffer.len().min(8192)].contains(&0) {
            return Ok("Binary file detected".to_string());
        }
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }

    async fn exists(&self, _path: &std::path::Path) -> bool {
        true
    }

    async fn is_dir(&self, _path: &std::path::Path) -> bool {
        false
    }

    async fn canonicalize(&self, path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
        Ok(path.to_path_buf())
    }

    async fn get_file_info(
        &self,
        _path: &std::path::Path,
    ) -> Option<crate::fs::provider::FileMetadata> {
        None
    }

    async fn get_permissions(&self, _path: &std::path::Path) -> Option<u32> {
        *self.permissions_result.lock().unwrap()
    }

    async fn set_permissions(&self, _path: &std::path::Path, _mode: u32) -> bool {
        true
    }

    async fn get_modified_time(&self, _path: &std::path::Path) -> Option<std::time::SystemTime> {
        None
    }

    async fn set_modified_time(
        &self,
        _path: &std::path::Path,
        _mtime: std::time::SystemTime,
    ) -> bool {
        true
    }

    fn context_key(&self) -> crate::fs::provider::ContextKey {
        crate::fs::provider::ContextKey::Ssh {
            user: "test".to_string(),
            host: "remote".to_string(),
            port: 22,
        }
    }

    fn display_path(&self, path: &std::path::Path) -> String {
        path.to_string_lossy().to_string()
    }

    #[allow(clippy::unused_async)]
    async fn calc_dir_size(&self, _path: &std::path::Path) -> anyhow::Result<u64> {
        Ok(0)
    }
}

fn create_test_app() -> AppState {
    AppState::test_default()
}

#[tokio::test]
async fn test_handle_remote_edit_event_cancel() {
    let mut app = create_test_app();

    let temp_path = std::env::temp_dir().join("test_edit_cancel.txt");
    tokio::fs::write(&temp_path, b"test content").await.unwrap();

    let mock_fs = Arc::new(MockFileSystem::new());
    app.popups.remote_edit = crate::state::RemoteEditState {
        is_visible: true,
        temp_guard: Some(TempFileGuard::new(temp_path.clone())),
        editor_child: None,
        remote_path: std::path::PathBuf::from("/remote/test.txt"),
        filename: "test.txt".to_string(),
        provider: mock_fs,
        original_checksum: md5::compute(b"test content").0,
        focused_button: 0,
    };

    let result = handle_remote_edit_event(termina::event::KeyCode::Escape, &mut app).await;

    assert!(!result);
    assert!(!app.popups.remote_edit.is_visible);
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn test_handle_remote_edit_event_upload_yes() {
    let mut app = create_test_app();

    let temp_path = std::env::temp_dir().join("test_edit_changed.txt");
    let original_content = b"original content";
    let changed_content = b"modified content";
    tokio::fs::write(&temp_path, changed_content).await.unwrap();

    let mock_fs = Arc::new(MockFileSystem::new());
    let original_checksum = md5::compute(original_content).0;
    app.popups.remote_edit = crate::state::RemoteEditState {
        is_visible: true,
        temp_guard: Some(TempFileGuard::new(temp_path.clone())),
        editor_child: None,
        remote_path: std::path::PathBuf::from("/remote/test.txt"),
        filename: "test.txt".to_string(),
        provider: mock_fs.clone(),
        original_checksum,
        focused_button: 1,
    };

    let result = handle_remote_edit_event(termina::event::KeyCode::Enter, &mut app).await;
    assert!(!result);
    assert!(!app.popups.remote_edit.is_visible);
    assert_eq!(
        mock_fs
            .write_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let written_data = mock_fs.write_data.lock().unwrap().clone().unwrap();
    assert_eq!(written_data, changed_content);
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn test_checksum_comparison_unchanged() {
    let content = b"test content for checksum";
    let checksum = md5::compute(content);
    let same_content = b"test content for checksum";
    let same_checksum = md5::compute(same_content);

    assert_eq!(checksum.0, same_checksum.0);
}

#[tokio::test]
async fn test_checksum_comparison_changed() {
    let content1 = b"original content";
    let content2 = b"modified content";
    let checksum1 = md5::compute(content1);
    let checksum2 = md5::compute(content2);

    assert_ne!(checksum1.0, checksum2.0);
}

#[tokio::test]
async fn test_checksum_empty_content() {
    let empty = b"";
    let checksum = md5::compute(empty);
    let empty_checksum = md5::compute(b"");
    assert_eq!(checksum.0, empty_checksum.0);
}

#[tokio::test]
async fn test_upload_edited_file_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path().join("test_upload.txt");
    let remote_path = std::path::PathBuf::from("/remote/test.txt");
    let content = b"uploaded content";

    tokio::fs::write(&temp_path, content).await.unwrap();

    let mock_fs = Arc::new(MockFileSystem::new());
    upload_edited_file(&remote_path, mock_fs.clone(), content)
        .await
        .unwrap();

    assert_eq!(
        mock_fs
            .write_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let written_data = mock_fs.write_data.lock().unwrap().clone().unwrap();
    assert_eq!(written_data, content);
}

#[tokio::test]
async fn test_upload_edited_file_with_permissions() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path().join("test_upload_perms.txt");
    let remote_path = std::path::PathBuf::from("/remote/test.txt");
    let content = b"content with perms";

    tokio::fs::write(&temp_path, content).await.unwrap();

    let mock_fs = Arc::new(MockFileSystem::new());
    mock_fs.permissions_result.lock().unwrap().replace(0o755);

    upload_edited_file(&remote_path, mock_fs.clone(), content)
        .await
        .unwrap();

    assert_eq!(
        mock_fs
            .write_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn test_upload_edited_file_failure() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path().join("test_upload_fail.txt");
    let remote_path = std::path::PathBuf::from("/remote/test.txt");
    let content = b"content";

    tokio::fs::write(&temp_path, content).await.unwrap();

    let mock_fs = Arc::new(MockFileSystem::new());
    mock_fs
        .write_error
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let result = upload_edited_file(&remote_path, mock_fs.clone(), content).await;

    assert!(result.is_err());
    assert!(temp_path.exists());
    tokio::fs::remove_file(&temp_path).await.unwrap();
}

#[tokio::test]
async fn test_handle_remote_edit_event_write_error_sets_error() {
    let mut app = create_test_app();

    let temp_path = std::env::temp_dir().join("test_edit_write_error.txt");
    let original_content = b"original content";
    let changed_content = b"modified content";
    tokio::fs::write(&temp_path, changed_content).await.unwrap();

    let mock_fs = Arc::new(MockFileSystem::new());
    mock_fs
        .write_error
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let original_checksum = md5::compute(original_content).0;
    app.popups.remote_edit = crate::state::RemoteEditState {
        is_visible: true,
        temp_guard: Some(TempFileGuard::new(temp_path.clone())),
        editor_child: None,
        remote_path: std::path::PathBuf::from("/remote/test.txt"),
        filename: "test.txt".to_string(),
        provider: mock_fs.clone(),
        original_checksum,
        focused_button: 1,
    };

    let result = handle_remote_edit_event(termina::event::KeyCode::Enter, &mut app).await;

    assert!(!result);
    assert!(!app.popups.remote_edit.is_visible);
    assert!(app.panels.left.active_tab().error.is_some());
    // The edited file is intentionally kept on failed upload
    assert!(temp_path.exists());
    tokio::fs::remove_file(&temp_path).await.unwrap();
}
