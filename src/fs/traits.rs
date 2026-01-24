use async_trait::async_trait;

#[async_trait]
pub trait FileSystem: Send + Sync {
    async fn try_exists(&self, path: &std::path::Path) -> anyhow::Result<bool>;
    async fn is_dir(&self, path: &std::path::Path) -> anyhow::Result<bool>;
    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()>;
    async fn read_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>>;
    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()>;
    async fn remove_file(&self, path: &std::path::Path) -> anyhow::Result<()>;
    async fn copy(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()>;
    async fn copy_with_progress(
        &self,
        src: &std::path::Path,
        dst: &std::path::Path,
        id: usize,
        tx: &tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
        cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<()>;
    async fn get_size(&self, path: &std::path::Path) -> anyhow::Result<u64>;
    async fn read_file(&self, path: &std::path::Path) -> anyhow::Result<Vec<u8>>;
    async fn read_chunk(
        &self,
        path: &std::path::Path,
        offset: u64,
        len: usize,
    ) -> anyhow::Result<Vec<u8>>;
    async fn write_file(&self, path: &std::path::Path, data: &[u8]) -> anyhow::Result<()>;
    async fn write_chunk(
        &self,
        path: &std::path::Path,
        offset: u64,
        data: &[u8],
    ) -> anyhow::Result<()>;
    async fn get_permissions(&self, path: &std::path::Path) -> Option<u32>;
    async fn set_permissions(&self, path: &std::path::Path, mode: u32) -> anyhow::Result<()>;
    async fn get_modified_time(&self, path: &std::path::Path) -> Option<std::time::SystemTime>;
    async fn set_modified_time(
        &self,
        path: &std::path::Path,
        mtime: std::time::SystemTime,
    ) -> anyhow::Result<()>;
    async fn write_file_with_permissions(
        &self,
        path: &std::path::Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> anyhow::Result<()>;
    fn context_key(&self) -> String;
}
