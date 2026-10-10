#[cfg(any(test, feature = "test-utils"))]
use crate::app::{AppState, Popups};
#[cfg(any(test, feature = "test-utils"))]
use crate::app_state::tabs::{IncrementalSearch, PanelSide, SortSettings, Tab, TabManager};
#[cfg(any(test, feature = "test-utils"))]
use crate::fs::local::LocalFs;
#[cfg(any(test, feature = "test-utils"))]
use crate::fs::provider::{FileMetadata, FileSystemProvider};
#[cfg(any(test, feature = "test-utils"))]
use crate::fs::utils::FileEntry;
#[cfg(any(test, feature = "test-utils"))]
use async_trait::async_trait;
#[cfg(any(test, feature = "test-utils"))]
use std::path::PathBuf;
#[cfg(any(test, feature = "test-utils"))]
use std::sync::Arc;
#[cfg(any(test, feature = "test-utils"))]
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn create_test_tab() -> Tab {
    Tab {
        provider: Arc::new(LocalFs::new()),
        current_dir: PathBuf::from("/test"),
        entries: vec![],
        cursor: 0,
        search: IncrementalSearch::default(),
        sort: SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        ssh_session_id: None,
        status_msg: None,
        dir_sizes: std::collections::HashMap::new(),
        is_reloading: false,
        visible_indices: Vec::new(),
        filter: crate::state::FileFilterState::new(),
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn create_test_tab_with_entries() -> Tab {
    let entries = vec![
        FileEntry {
            name: "..".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
        FileEntry {
            name: "dir1".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: "drwxr-xr-x".to_string(),
            selected: false,
        },
        FileEntry {
            name: "file1.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(100),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        },
        FileEntry {
            name: "file2.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(200),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        },
    ];

    Tab {
        provider: Arc::new(LocalFs::new()),
        current_dir: PathBuf::from("/tmp"),
        entries,
        cursor: 0,
        search: IncrementalSearch::default(),
        sort: SortSettings::default(),
        scroll_offset: 0,
        error: None,
        custom_title: None,
        ssh_session_id: None,
        status_msg: None,
        dir_sizes: std::collections::HashMap::new(),
        is_reloading: false,
        visible_indices: Vec::new(),
        filter: crate::state::FileFilterState::new(),
    }
}

/// Builder for constructing `AppState` instances in tests.
///
/// Centralizes the full `AppState` struct literal so tests only override the
/// pieces they need (tabs, task channel, opener) instead of duplicating every
/// field. Any field added to `AppState` only needs to be handled here.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
pub struct TestAppBuilder {
    left: Option<TabManager>,
    right: Option<TabManager>,
    task_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::tasks::UiEvent>>,
    opener: Option<std::sync::Arc<dyn crate::opener::FileOpener + Send + Sync>>,
}

#[cfg(any(test, feature = "test-utils"))]
impl TestAppBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn left(mut self, manager: TabManager) -> Self {
        self.left = Some(manager);
        self
    }

    #[must_use]
    pub fn right(mut self, manager: TabManager) -> Self {
        self.right = Some(manager);
        self
    }

    #[must_use]
    pub fn task_tx(
        mut self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::tasks::UiEvent>,
    ) -> Self {
        self.task_tx = Some(tx);
        self
    }

    #[must_use]
    pub fn opener(
        mut self,
        opener: std::sync::Arc<dyn crate::opener::FileOpener + Send + Sync>,
    ) -> Self {
        self.opener = Some(opener);
        self
    }

    /// Builds the application state.
    ///
    /// # Panics
    ///
    /// Panics if `DirectoryHistory` or `SshConnectionHistory` fails to initialize.
    #[must_use]
    pub fn build(self) -> AppState {
        let test_tab = create_test_tab();

        AppState {
            panels: crate::app::PanelState {
                left: self.left.unwrap_or_else(|| TabManager {
                    tabs: vec![test_tab.clone()],
                    active_tab_index: 0,
                }),
                right: self.right.unwrap_or_else(|| TabManager {
                    tabs: vec![test_tab],
                    active_tab_index: 0,
                }),
                active: PanelSide::Left,
            },
            file_viewer: crate::state::FileViewerState::new(false, "test-theme"),
            fuzzy_search: crate::ui::fuzzy_search_ui::FuzzySearchState::new(),
            popups: Popups::new(),
            tasks: crate::app::TaskState {
                task_manager: crate::tasks::TaskManager::new(
                    self.task_tx
                        .unwrap_or_else(|| tokio::sync::mpsc::unbounded_channel().0),
                ),
                show_task_manager: false,
            },
            ssh_manager: Arc::new(crate::ssh_manager::SshManager::default()),
            dir_history: crate::dir_history::DirectoryHistory::test_default(),
            watcher: None,
            remote_watcher: None,
            input_polling_handle: None,
            keyboard: crate::config::KeyboardConfig::default(),
            global: crate::config::GlobalConfig {
                mouse: Some(false),
                ..crate::config::GlobalConfig::default()
            },
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::test_default(),
            bookmark_store: crate::bookmarks::BookmarkStore::test_default(),
            os: crate::app::OsServices {
                clipboard: Box::new(crate::clipboard::InMemoryFileClipboard::new()),
                opener: self
                    .opener
                    .unwrap_or_else(|| std::sync::Arc::new(crate::opener::SystemOpener)),
            },
            cache: crate::app::CacheState::default(),
            layout: crate::layout::LayoutState::default(),
            pending_action: None,
            mouse: crate::app::MouseState::default(),
            remote_reloaded_at: std::collections::HashMap::new(),
        }
    }
}

/// Creates a test application state using the default builder.
///
/// # Panics
///
/// Panics if `DirectoryHistory` or `SshConnectionHistory` fails to initialize.
#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn create_test_app() -> AppState {
    TestAppBuilder::new().build()
}

/// Test-support filesystem: async `tokio::fs` implementation of the unified
/// `FileSystemProvider` trait, used by unit tests that need a real provider.
#[cfg(any(test, feature = "test-utils"))]
pub struct StdFileSystem;

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl FileSystemProvider for StdFileSystem {
    async fn list_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<FileEntry>> {
        let mut result = Vec::new();
        result.push(FileEntry {
            name: "..".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        });
        let mut rd = tokio::fs::read_dir(path)
            .await
            .map_err(anyhow::Error::from)?;
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().await.is_ok_and(|t| t.is_dir());
            let size = if is_dir {
                None
            } else {
                entry.metadata().await.ok().map(|m| m.len())
            };
            result.push(FileEntry {
                name,
                is_dir,
                is_symlink: false,
                size,
                modified: None,
                attributes: String::new(),
                selected: false,
            });
        }
        Ok(result)
    }

    async fn create_dir(&self, path: &std::path::Path) -> anyhow::Result<()> {
        tokio::fs::create_dir(path)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(path)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    async fn create_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        tokio::fs::File::create(path)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    async fn delete(&self, path: &std::path::Path, recursive: bool) -> anyhow::Result<()> {
        if path.is_dir() {
            if recursive {
                tokio::fs::remove_dir_all(path)
                    .await
                    .map_err(anyhow::Error::from)?;
            } else {
                tokio::fs::remove_dir(path)
                    .await
                    .map_err(anyhow::Error::from)?;
            }
        } else {
            tokio::fs::remove_file(path)
                .await
                .map_err(anyhow::Error::from)?;
        }
        Ok(())
    }

    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        tokio::fs::rename(src, dst)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    async fn read_file(&self, path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
        Ok(tokio::fs::read(path).await.map_err(anyhow::Error::from)?)
    }

    async fn read_file_at(
        &self,
        path: &std::path::Path,
        offset: u64,
        len: usize,
    ) -> anyhow::Result<Vec<u8>> {
        let mut file = tokio::fs::File::open(path).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut buffer = vec![0; len];
        let n = file.read(&mut buffer).await?;
        buffer.truncate(n);
        Ok(buffer)
    }

    async fn write_file(&self, path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
        tokio::fs::write(path, data).await?;
        Ok(())
    }

    async fn write_file_at(
        &self,
        path: &std::path::Path,
        offset: u64,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        file.write_all(data).await?;
        Ok(())
    }

    async fn exists(&self, path: &std::path::Path) -> bool {
        tokio::fs::try_exists(path).await.unwrap_or(false)
    }

    async fn is_dir(&self, path: &std::path::Path) -> bool {
        path.is_dir()
    }

    async fn canonicalize(&self, path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
        Ok(tokio::fs::canonicalize(path)
            .await
            .map_err(anyhow::Error::from)?)
    }

    async fn get_file_info(&self, path: &std::path::Path) -> Option<FileMetadata> {
        let meta = tokio::fs::metadata(path).await.ok()?;
        #[cfg(unix)]
        let permissions: Option<u32> = {
            use std::os::unix::fs::PermissionsExt;
            Some(meta.permissions().mode() & 0o777)
        };
        #[cfg(not(unix))]
        let permissions: Option<u32> = None;
        Some(FileMetadata {
            size: meta.len(),
            modified: meta.modified().ok(),
            permissions,
        })
    }

    async fn get_permissions(&self, path: &std::path::Path) -> Option<u32> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::metadata(path)
                .await
                .ok()
                .map(|m| m.permissions().mode() & 0o777)
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            None
        }
    }

    async fn set_permissions(&self, path: &std::path::Path, mode: u32) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let Ok(metadata) = tokio::fs::metadata(path).await else {
                return false;
            };
            let current_mode = metadata.permissions().mode();
            let new_mode = (current_mode & !0o777) | (mode & 0o777);
            let mut perms = metadata.permissions();
            perms.set_mode(new_mode);
            tokio::fs::set_permissions(path, perms).await.is_ok()
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            let _ = mode;
            false
        }
    }

    async fn get_modified_time(&self, path: &std::path::Path) -> Option<std::time::SystemTime> {
        tokio::fs::metadata(path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
    }

    async fn set_modified_time(
        &self,
        path: &std::path::Path,
        mtime: std::time::SystemTime,
    ) -> bool {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            #[cfg(unix)]
            {
                let duration = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| std::io::Error::other("invalid mtime"))?;
                let sec = duration.as_secs();
                let sec = i64::try_from(sec).unwrap_or(i64::MAX);
                let _sec = sec as libc::time_t;
                let _nsec = libc::c_long::from(duration.subsec_nanos());
                let path_cstr =
                    std::ffi::CString::new(path.to_string_lossy().as_bytes()).map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path")
                    })?;
                let result = unsafe {
                    libc::utimensat(
                        libc::AT_FDCWD,
                        path_cstr.as_ptr(),
                        [
                            libc::timespec {
                                tv_sec: 0,
                                tv_nsec: libc::UTIME_OMIT,
                            },
                            libc::timespec {
                                tv_sec: _sec,
                                tv_nsec: _nsec,
                            },
                        ]
                        .as_ptr(),
                        0,
                    )
                };
                if result != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            #[cfg(windows)]
            {
                use filetime::FileTime;
                let ft = FileTime::from_system_time(mtime);
                filetime::set_file_mtime(&path, ft)?;
            }
            Ok::<(), std::io::Error>(())
        })
        .await
        .is_ok_and(|r| r.is_ok())
    }

    fn display_prefix(&self) -> &'static str {
        ""
    }

    fn is_local(&self) -> bool {
        true
    }

    fn context_key(&self) -> crate::fs::provider::ContextKey {
        crate::fs::provider::ContextKey::Local
    }

    fn display_path(&self, path: &std::path::Path) -> String {
        path.to_string_lossy().to_string()
    }

    async fn calc_dir_size(&self, path: &std::path::Path) -> anyhow::Result<u64> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let mut total = 0u64;
            for entry in walkdir::WalkDir::new(&path) {
                let entry = entry?;
                if entry.file_type().is_file() {
                    total += entry.metadata()?.len();
                }
            }
            Ok::<u64, anyhow::Error>(total)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }
}
