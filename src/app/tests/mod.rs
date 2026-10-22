//! Test suite for `app`.
mod panels;
mod persistence;
mod popups;
mod reload;
mod tabs;

use crate::app::{AppState, PopupKind};
use crate::app_state::tabs::{
    IncrementalSearch, PanelSide, SortColumn, SortDirection, SortSettings, Tab, TabManager,
};
use crate::fs::provider::{FileMetadata, FileSystemProvider};
use crate::fs::utils::FileEntry;
use crate::state::{CreateFileState, HelpState};
use crate::ui::text_edit::TextEditState;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use termina::event::{KeyCode, Modifiers};

fn create_test_tab() -> Tab {
    crate::test_utils::create_test_tab_with_entries()
}

struct MockProvider {
    local: bool,
    host: &'static str,
}

impl MockProvider {
    fn remote(host: &'static str) -> Self {
        Self { local: false, host }
    }

    #[cfg(windows)]
    fn local() -> Self {
        Self {
            local: true,
            host: "",
        }
    }
}

/// A tab backed by a remote [`MockProvider`] for `host`, so that contexts of
/// different tabs differ.
fn remote_tab(host: &'static str) -> Tab {
    Tab {
        provider: Arc::new(MockProvider::remote(host)),
        current_dir: PathBuf::from(format!("/{host}")),
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
        show_hidden: true,
    }
}

#[async_trait::async_trait]
impl FileSystemProvider for MockProvider {
    async fn list_dir(&self, _path: &Path) -> anyhow::Result<Vec<FileEntry>> {
        Ok(vec![])
    }
    async fn create_dir(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn create_dir_all(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn create_file(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn delete(&self, _path: &Path, _recursive: bool) -> anyhow::Result<()> {
        Ok(())
    }
    async fn rename(&self, _from: &Path, _to: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    async fn read_file(&self, _path: &Path) -> anyhow::Result<Vec<u8>> {
        Ok(vec![])
    }
    async fn read_file_at(
        &self,
        _path: &Path,
        _offset: u64,
        _len: usize,
    ) -> anyhow::Result<Vec<u8>> {
        Ok(vec![])
    }
    async fn write_file(&self, _path: &Path, _data: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
    async fn write_file_at(&self, _path: &Path, _offset: u64, _data: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
    fn display_prefix(&self) -> &'static str {
        ""
    }
    fn is_local(&self) -> bool {
        self.local
    }
    async fn exists(&self, _path: &Path) -> bool {
        true
    }
    async fn is_dir(&self, _path: &Path) -> bool {
        true
    }
    async fn canonicalize(&self, path: &Path) -> anyhow::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
    async fn get_file_info(&self, _path: &Path) -> Option<FileMetadata> {
        None
    }
    async fn get_permissions(&self, _path: &Path) -> Option<u32> {
        None
    }
    async fn set_permissions(&self, _path: &Path, _mode: u32) -> bool {
        false
    }
    async fn get_modified_time(&self, _path: &Path) -> Option<std::time::SystemTime> {
        None
    }
    async fn set_modified_time(&self, _path: &Path, _mtime: std::time::SystemTime) -> bool {
        false
    }
    fn context_key(&self) -> crate::fs::provider::ContextKey {
        if self.local {
            crate::fs::provider::ContextKey::Local
        } else {
            crate::fs::provider::ContextKey::Ssh {
                user: "test".to_string(),
                host: self.host.to_string(),
                port: 22,
            }
        }
    }
    fn display_path(&self, path: &Path) -> String {
        path.to_string_lossy().to_string()
    }
    #[allow(clippy::unused_async)]
    async fn calc_dir_size(&self, _path: &Path) -> anyhow::Result<u64> {
        Ok(0)
    }
}
