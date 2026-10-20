//! `FileSystemProvider` implementation for the SFTP backend.

use super::SftpFs;
use super::transfer;
use crate::fs::provider::{
    BackendKind, Capabilities, FileMetadata, FileSystemProvider, TaskProgressContext,
};
use crate::fs::utils::{FileEntry, normalize_sftp_path};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use secrecy::SecretString;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[async_trait]
impl FileSystemProvider for SftpFs {
    // ---- directory listing ------------------------------------------------

    async fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        transfer::build_dir_listing(&self.sftp, path).await
    }

    async fn read_link(&self, path: &Path) -> Option<PathBuf> {
        let p = normalize_sftp_path(path);
        self.sftp.read_link(p).await.ok().map(PathBuf::from)
    }

    async fn create_dir(&self, path: &Path) -> Result<()> {
        let sftp = &*self.sftp;
        let p = normalize_sftp_path(path);
        sftp.create_dir(p)
            .await
            .map_err(|e| anyhow!("Failed to create directory: {e}"))
    }

    async fn create_dir_all(&self, path: &Path) -> Result<()> {
        let sftp = &*self.sftp;
        let full = normalize_sftp_path(path);
        let mut current = String::new();
        for comp in full.split('/') {
            if comp.is_empty() {
                continue;
            }
            if current.is_empty() {
                current = comp.to_string();
            } else {
                current.push('/');
                current.push_str(comp);
            }

            match sftp.metadata(&current).await {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(anyhow!("{current} exists and is not a directory")),
                Err(_) => {
                    if let Err(e) = sftp.create_dir(current.clone()).await {
                        let meta = sftp
                            .metadata(&current)
                            .await
                            .map_err(|e| anyhow!("Failed to create directory {current}: {e}"))?;
                        if !meta.is_dir() {
                            return Err(anyhow!("{current} exists and is not a directory"));
                        }
                        let _ = e; // "already exists" — safe to continue
                    }
                }
            }
        }
        Ok(())
    }

    async fn create_file(&self, path: &Path) -> Result<()> {
        let sftp = &*self.sftp;
        let p = normalize_sftp_path(path);
        sftp.create(p)
            .await
            .map_err(|e| anyhow!("Failed to create file: {e}"))?;
        Ok(())
    }

    async fn create_file_exclusive(&self, path: &Path) -> Result<bool> {
        let sftp = &*self.sftp;
        let p = normalize_sftp_path(path);
        // SFTP v3 reports an existing EXCLUDE target as a generic Failure, so
        // pre-check to turn that into a clean "name taken"; the open's EXCLUDE
        // flag still guards the create itself.
        if sftp.try_exists(p.clone()).await.unwrap_or(false) {
            return Ok(false);
        }
        sftp.open_with_flags(
            p,
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::TRUNCATE,
        )
        .await
        .map(|_| true)
        .map_err(|e| anyhow!("Failed to create file exclusively: {e}"))
    }

    async fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        let sftp = &*self.sftp;
        let p = normalize_sftp_path(path);
        let meta = sftp
            .metadata(&p)
            .await
            .map_err(|e| anyhow!("Failed to stat path: {e}"))?;
        if meta.is_dir() {
            if recursive {
                transfer::delete_recursive(sftp, &p).await
            } else {
                sftp.remove_dir(p)
                    .await
                    .map_err(|e| anyhow!("Failed to remove directory: {e}"))
            }
        } else {
            sftp.remove_file(p)
                .await
                .map_err(|e| anyhow!("Failed to remove file: {e}"))
        }
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let sftp = &*self.sftp;
        sftp.rename(normalize_sftp_path(from), normalize_sftp_path(to))
            .await
            .map_err(|e| anyhow!("Failed to rename: {e}"))
    }

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let sftp = &*self.sftp;
        sftp.read(normalize_sftp_path(path))
            .await
            .map_err(|e| anyhow!("Failed to read file: {e}"))
    }

    async fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        let sftp = &*self.sftp;
        let p = normalize_sftp_path(path);
        let mut file = sftp
            .open(p)
            .await
            .map_err(|e| anyhow!("Failed to open file: {e}"))?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| anyhow!("Failed to seek: {e}"))?;
        let mut buf = vec![0u8; len];
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| anyhow!("Failed to read: {e}"))?;
        buf.truncate(n);
        Ok(buf)
    }

    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        let sftp = &*self.sftp;
        sftp.write(normalize_sftp_path(path), data)
            .await
            .map_err(|e| anyhow!("Failed to write file: {e}"))
    }

    async fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
        let sftp = &*self.sftp;
        let p = normalize_sftp_path(path);
        if offset == 0 {
            sftp.write(p, data)
                .await
                .map_err(|e| anyhow!("Failed to write file: {e}"))
        } else {
            let mut file = sftp
                .open_with_flags(p, OpenFlags::READ | OpenFlags::WRITE)
                .await
                .map_err(|e| anyhow!("Failed to open file for write-at: {e}"))?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| anyhow!("Failed to seek: {e}"))?;
            file.write_all(data)
                .await
                .map_err(|e| anyhow!("Failed to write at offset {offset}: {e}"))?;
            Ok(())
        }
    }

    fn display_prefix(&self) -> &str {
        &self.display_prefix
    }

    fn is_local(&self) -> bool {
        false
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::for_kind(BackendKind::Remote)
    }

    async fn exists(&self, path: &Path) -> bool {
        let sftp = &*self.sftp;
        sftp.try_exists(normalize_sftp_path(path))
            .await
            .unwrap_or(false)
    }

    async fn is_dir(&self, path: &Path) -> bool {
        let sftp = &*self.sftp;
        let Ok(meta) = sftp.metadata(normalize_sftp_path(path)).await else {
            return false;
        };
        meta.is_dir()
    }

    async fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        let sftp = &*self.sftp;
        let s = sftp
            .canonicalize(normalize_sftp_path(path))
            .await
            .map_err(|e| anyhow!("Failed to canonicalize: {e}"))?;
        Ok(PathBuf::from(s))
    }

    async fn get_file_info(&self, path: &Path) -> Option<FileMetadata> {
        let sftp = &*self.sftp;
        let Ok(meta) = sftp.metadata(normalize_sftp_path(path)).await else {
            return None;
        };
        Some(FileMetadata {
            size: meta.size.unwrap_or(0),
            modified: meta
                .mtime
                .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(t))),
            permissions: meta.permissions.map(|p| p & 0o777),
        })
    }

    async fn get_permissions(&self, path: &Path) -> Option<u32> {
        let sftp = &*self.sftp;
        let Ok(meta) = sftp.metadata(normalize_sftp_path(path)).await else {
            return None;
        };
        meta.permissions.map(|p| p & 0o777)
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        let sftp = &*self.sftp;
        let p = normalize_sftp_path(path);
        sftp.set_metadata(
            p,
            FileAttributes {
                size: None,
                uid: None,
                gid: None,
                user: None,
                group: None,
                permissions: Some(mode),
                atime: None,
                mtime: None,
            },
        )
        .await
        .is_ok()
    }

    async fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        let sftp = &*self.sftp;
        let Ok(meta) = sftp.metadata(normalize_sftp_path(path)).await else {
            return None;
        };
        meta.mtime
            .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(t)))
    }

    async fn set_modified_time(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
        let sftp = &*self.sftp;
        let secs = mtime
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX));
        sftp.set_metadata(
            normalize_sftp_path(path),
            FileAttributes {
                size: None,
                uid: None,
                gid: None,
                user: None,
                group: None,
                permissions: None,
                atime: None,
                mtime: Some(secs),
            },
        )
        .await
        .is_ok()
    }

    fn context_key(&self) -> crate::fs::provider::ContextKey {
        crate::fs::provider::ContextKey::Ssh {
            user: self.user.clone(),
            host: self.host.clone(),
            port: self.port,
        }
    }

    fn get_password(&self) -> Option<SecretString> {
        self.password.clone()
    }

    fn get_host(&self) -> Option<&str> {
        Some(&self.host)
    }

    fn get_user(&self) -> Option<&str> {
        Some(&self.user)
    }

    fn get_port(&self) -> u16 {
        self.port
    }

    fn display_path(&self, path: &Path) -> String {
        normalize_sftp_path(path)
    }

    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64> {
        transfer::calc_dir_size(self, path).await
    }

    async fn copy_to_local(
        &self,
        src: &Path,
        dest_fs: &dyn FileSystemProvider,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        transfer::copy_to_local(self, src, dest_fs, dest, progress).await
    }

    async fn copy_from_local(
        &self,
        src_fs: &dyn FileSystemProvider,
        src: &Path,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        transfer::copy_from_local(self, src_fs, src, dest, progress).await
    }

    async fn supports_copy_to_local(&self, src: &Path, dest_fs: &dyn FileSystemProvider) -> bool {
        transfer::supports_copy_to_local(self, src, dest_fs).await
    }
}
