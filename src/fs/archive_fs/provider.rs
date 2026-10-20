//! The `FileSystemProvider` implementation for `ArchiveFs`.

use super::ArchiveFs;
use crate::fs::provider::{
    BackendKind, Capabilities, FileMetadata, FileSystemProvider, TaskProgressContext,
};
use crate::fs::utils::FileEntry;
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

#[async_trait]
impl FileSystemProvider for ArchiveFs {
    async fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        let fs = self.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || fs.list_dir_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?)
    }

    async fn read_link(&self, path: &Path) -> Option<PathBuf> {
        let normalized = Self::resolve_internal_path(path);
        self.symlinks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&normalized)
            .cloned()
    }

    async fn create_dir(&self, path: &Path) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.create_dir_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn create_dir_all(&self, path: &Path) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.create_dir_all_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?;
        Ok(())
    }

    async fn create_file(&self, path: &Path) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.create_file_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.delete_sync(&path, recursive))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let fs = self.clone();
        let from = from.to_path_buf();
        let to = to.to_path_buf();
        tokio::task::spawn_blocking(move || fs.rename_sync(&from, &to))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.read_file_sync(&path))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.read_file_at_sync(&path, offset, len))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || fs.write_file_sync(&path, &data))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    async fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
        let fs = self.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || fs.write_file_at_sync(&path, offset, &data))
            .await
            .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
    }

    fn display_prefix(&self) -> &'static str {
        ""
    }

    fn is_local(&self) -> bool {
        false
    }

    fn is_archive(&self) -> bool {
        true
    }

    fn capabilities(&self) -> Capabilities {
        // A format that cannot be rewritten (tar, gzip, rpm) has no way to
        // accept a new entry; report that up front so a copy into it is refused
        // before it writes anything. `is_writable` and any UI gate read this
        // single field.
        Capabilities::for_kind(BackendKind::Archive).with_writable(self.handler.is_writable())
    }

    fn archive_path(&self) -> Option<PathBuf> {
        Some(self.archive_path.clone())
    }

    async fn exists(&self, path: &Path) -> bool {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.exists_sync(&path))
            .await
            .unwrap_or(false)
    }

    async fn is_dir(&self, path: &Path) -> bool {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.is_dir_sync(&path))
            .await
            .unwrap_or(false)
    }

    async fn stat_path(&self, path: &Path) -> Option<bool> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let p = fs.resolve_path(&path);
            if p == Path::new(".") {
                return Some(true);
            }
            fs.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&p)
                .map(|e| e.is_dir)
        })
        .await
        .unwrap_or(None)
    }

    async fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        let fs = self.clone();
        let path = path.to_path_buf();
        Ok(
            tokio::task::spawn_blocking(move || fs.canonicalize_sync(&path))
                .await
                .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?,
        )
    }

    async fn get_file_info(&self, path: &Path) -> Option<FileMetadata> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.get_file_info_sync(&path))
            .await
            .ok()
            .flatten()
    }

    async fn get_permissions(&self, path: &Path) -> Option<u32> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.get_permissions_sync(&path))
            .await
            .ok()
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.set_permissions_sync(&path, mode))
            .await
            .unwrap_or(false)
    }

    async fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.get_modified_time_sync(&path))
            .await
            .ok()
            .flatten()
    }

    async fn set_modified_time(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
        let fs = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || fs.set_modified_time_sync(&path, mtime))
            .await
            .unwrap_or(false)
    }

    fn context_key(&self) -> crate::fs::provider::ContextKey {
        crate::fs::provider::ContextKey::Archive(self.archive_path.clone())
    }

    fn display_path(&self, path: &Path) -> String {
        if path.has_root() {
            path.to_string_lossy().to_string()
        } else {
            format!("/{}", path.to_string_lossy())
        }
    }

    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64> {
        let fs = self.clone();
        let path = path.to_path_buf();
        Ok(
            tokio::task::spawn_blocking(move || fs.calc_dir_size_sync(&path))
                .await
                .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?,
        )
    }

    async fn copy_from_local(
        &self,
        src_fs: &dyn FileSystemProvider,
        src: &Path,
        dest: &Path,
        _progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        if !src_fs.capabilities().is_local() {
            return None;
        }

        let src = src.to_path_buf();
        let dest = dest.to_path_buf();
        let handler = self.handler.clone();
        let this = self.clone();
        let src_fs_is_local = src_fs.capabilities().is_local();

        Some(
            tokio::task::spawn_blocking(move || {
                let rel_dest = if dest.has_root() {
                    dest.strip_prefix("/").unwrap_or(&dest)
                } else {
                    &dest
                };
                let dest_str = rel_dest.to_string_lossy().replace('\\', "/");
                let dest_str = dest_str.trim_end_matches('/').to_string();

                if src_fs_is_local && src.is_dir() {
                    let mut files_to_add = Vec::new();
                    let mut dirs_to_add = Vec::new();

                    // The destination directory itself (walkdir below skips the root).
                    if !dest_str.is_empty() {
                        dirs_to_add.push((dest_str.clone(), src.metadata()?.modified().ok()));
                    }

                    // Walk directory and collect files/dirs. Links are followed
                    // so a linked directory (and links inside the tree) copy
                    // their contents, matching a normal recursive copy. Without
                    // this, the root of a linked directory is reported as a
                    // link rather than a directory and lands in the file list
                    // with the destination's own (possibly empty) name.
                    for entry in walkdir::WalkDir::new(&src).follow_links(true) {
                        let entry = entry?;
                        let rel_path = entry.path().strip_prefix(&src)?;
                        let mut entry_dest = dest_str.clone();
                        if !rel_path.as_os_str().is_empty() {
                            if !entry_dest.is_empty() {
                                entry_dest.push('/');
                            }
                            entry_dest.push_str(&rel_path.to_string_lossy().replace('\\', "/"));
                        }

                        if entry.file_type().is_dir() {
                            if !rel_path.as_os_str().is_empty() {
                                dirs_to_add.push((entry_dest, entry.metadata()?.modified().ok()));
                            }
                        } else {
                            files_to_add.push((entry.path().to_path_buf(), entry_dest));
                        }
                    }

                    // Batch add directories and files in a single rewrite
                    let dirs_refs: Vec<(&str, Option<std::time::SystemTime>)> =
                        dirs_to_add.iter().map(|(d, m)| (d.as_str(), *m)).collect();
                    let files_refs: Vec<(&Path, &str)> = files_to_add
                        .iter()
                        .map(|(p, d)| (p.as_path(), d.as_str()))
                        .collect();
                    handler.add_files_and_directories(&files_refs, &dirs_refs)?;
                } else {
                    handler.add_file(&src, &dest_str)?;
                }

                this.scan_archive()?;
                Ok(())
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("Join error: {e}"))),
        )
    }

    async fn supports_copy_to_local(&self, _src: &Path, dest_fs: &dyn FileSystemProvider) -> bool {
        dest_fs.capabilities().is_local()
    }

    /// Copies from any provider (another archive, a remote host) by decoding
    /// entries through the provider API and appending them to this archive.
    async fn copy_from_source(
        &self,
        src_fs: &dyn FileSystemProvider,
        src: &Path,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        Some(
            self.copy_from_source_into(src_fs, src, dest, progress)
                .await,
        )
    }

    async fn copy_to_local(
        &self,
        src: &Path,
        dest_fs: &dyn FileSystemProvider,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        if !dest_fs.capabilities().is_local() {
            return None;
        }

        let src = src.to_path_buf();
        let dest = dest.to_path_buf();
        let progress = progress.clone();
        // A symlink to a directory is enterable, but extracting it must create
        // the link itself, not descend into the target: pass `is_dir = false`
        // so the format handlers treat it as a single entry (as `tar` does).
        let is_dir = self.is_dir_sync(&src) && !self.is_symlink_sync(&src);
        let handler = self.handler.clone();

        Some(
            tokio::task::spawn_blocking(move || {
                let rel_src = if src.has_root() {
                    src.strip_prefix("/").unwrap_or(&src)
                } else {
                    &src
                };
                let src_str = rel_src.to_string_lossy().replace('\\', "/");
                let src_str = src_str.trim_end_matches('/').to_string();

                handler.extract(&src_str, &dest, is_dir, &progress)
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("Join error: {e}"))),
        )
    }
}
