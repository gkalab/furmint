use super::traits::FileSystem;
use async_trait::async_trait;
use secrecy::SecretString;
use std::path::Path;

pub struct ProviderFileSystem(pub std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>);

#[async_trait]
impl FileSystem for ProviderFileSystem {
    async fn try_exists(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || p.exists(&path)).await?)
    }
    async fn is_dir(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || p.is_dir(&path)).await?)
    }
    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || p.create_dir(&path)).await??;
        Ok(())
    }
    async fn read_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let p = self.0.clone();
        let path_buf = path.to_path_buf();
        let path_buf_clone = path_buf.clone();
        let entries = tokio::task::spawn_blocking(move || p.list_dir(&path_buf)).await??;
        Ok(entries
            .into_iter()
            .filter(|e| e.name != "..")
            .map(|e| path_buf_clone.join(e.name))
            .collect())
    }
    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        let p = self.0.clone();
        let src = src.to_path_buf();
        let dst = dst.to_path_buf();
        tokio::task::spawn_blocking(move || p.rename(&src, &dst)).await??;
        Ok(())
    }
    async fn remove_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || p.delete(&path, false)).await??;
        Ok(())
    }
    async fn copy(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        // We use a dummy cancel and tx for the basic copy if needed,
        // but it's better to just implement it with read/write if small.
        // Actually, let's just use copy_with_progress with a blackhole channel if we must.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.copy_with_progress(src, dst, 0, &tx, &cancel).await
    }

    async fn get_size(&self, path: &std::path::Path) -> anyhow::Result<u64> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || {
            let parent = path.parent().unwrap_or(Path::new("/"));
            let entries = p.list_dir(parent)?;
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let entry = entries
                .iter()
                .find(|e| e.name == name)
                .ok_or_else(|| anyhow::anyhow!("File not found: {}", path.display()))?;
            Ok::<u64, anyhow::Error>(entry.size.unwrap_or(0))
        })
        .await??)
    }

    async fn copy_with_progress(
        &self,
        src: &std::path::Path,
        dst: &std::path::Path,
        id: usize,
        tx: &tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
        cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<()> {
        let provider = self.0.clone();
        let src_buf = src.to_path_buf();
        let dst_buf = dst.to_path_buf();
        let tx = tx.clone();
        let cancel = cancel.clone();

        tokio::task::spawn_blocking(move || {
            // we need to get total_size first
            let entries = provider.list_dir(src_buf.parent().unwrap_or(Path::new("/")))?;
            let entry = entries
                .iter()
                .find(|e| e.name == src_buf.file_name().unwrap_or_default().to_string_lossy())
                .ok_or_else(|| anyhow::anyhow!("Source file not found in directory listing"))?;
            let total_size = entry.size.unwrap_or(0);
            let mtime = entry.modified;
            let perms = provider.get_permissions(&src_buf);

            let data = provider.read_file(&src_buf)?;
            let processed = data.len() as u64;

            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(());
            }

            provider.write_file_with_permissions(&dst_buf, &data, perms)?;

            let _ = tx.send(crate::tasks::TaskEvent::UpdateByteProgress(
                id, processed, total_size,
            ));

            if let Some(mt) = mtime {
                provider.set_modified_time(&dst_buf, mt);
            }

            Ok::<(), anyhow::Error>(())
        })
        .await??;

        Ok(())
    }
    async fn read_file(&self, path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || p.read_file(&path)).await??)
    }
    async fn read_chunk(
        &self,
        path: &std::path::Path,
        offset: u64,
        len: usize,
    ) -> anyhow::Result<Vec<u8>> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || p.read_file_at(&path, offset, len)).await??)
    }
    async fn write_file(&self, path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || p.write_file(&path, &data)).await??;
        Ok(())
    }
    async fn write_chunk(
        &self,
        path: &std::path::Path,
        offset: u64,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || p.write_file_at(&path, offset, &data)).await??;
        Ok(())
    }

    async fn get_permissions(&self, path: &std::path::Path) -> Option<u32> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || p.get_permissions(&path))
            .await
            .unwrap_or(None)
    }
    async fn set_permissions(&self, path: &std::path::Path, mode: u32) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        let success = tokio::task::spawn_blocking(move || p.set_permissions(&path, mode))
            .await
            .unwrap_or(false);
        if success {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to set permissions"))
        }
    }

    async fn get_modified_time(&self, path: &std::path::Path) -> Option<std::time::SystemTime> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || p.get_modified_time(&path))
            .await
            .ok()
            .flatten()
    }

    async fn set_modified_time(
        &self,
        path: &std::path::Path,
        mtime: std::time::SystemTime,
    ) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        let success = tokio::task::spawn_blocking(move || p.set_modified_time(&path, mtime))
            .await
            .unwrap_or(false);
        if success {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to set modified time"))
        }
    }

    async fn write_file_with_permissions(
        &self,
        path: &std::path::Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || p.write_file_with_permissions(&path, &data, mode))
            .await??;
        Ok(())
    }

    fn context_key(&self) -> String {
        self.0.context_key()
    }

    fn is_local(&self) -> bool {
        self.0.is_local()
    }

    fn is_archive(&self) -> bool {
        self.0.is_archive()
    }

    fn get_password(&self) -> Option<SecretString> {
        self.0.get_password()
    }

    async fn copy_to_local(
        &self,
        src: &std::path::Path,
        dest_fs: &dyn FileSystem,
        dest: &std::path::Path,
        progress: &super::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        self.0.copy_to_local(src, dest_fs, dest, progress).await
    }

    async fn copy_from_local(
        &self,
        src_fs: &dyn FileSystem,
        src: &std::path::Path,
        dest: &std::path::Path,
        progress: &super::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        self.0.copy_from_local(src_fs, src, dest, progress).await
    }
}
