use super::traits::FileSystem;
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[allow(dead_code)]
pub struct StdFileSystem;

#[async_trait]
impl FileSystem for StdFileSystem {
    async fn try_exists(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        Ok(tokio::fs::try_exists(path).await?)
    }
    async fn is_dir(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        Ok(path.is_dir())
    }
    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::create_dir_all(path).await?)
    }
    async fn read_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let mut result = Vec::new();
        let mut rd = tokio::fs::read_dir(path).await?;
        while let Ok(Some(entry)) = rd.next_entry().await {
            result.push(entry.path());
        }
        Ok(result)
    }
    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::rename(src, dst).await?)
    }
    async fn remove_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::remove_file(path).await?)
    }
    async fn copy(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        let mtime = tokio::fs::metadata(src)
            .await
            .ok()
            .and_then(|m| m.modified().ok());
        tokio::fs::copy(src, dst)
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from)?;
        if let Some(mt) = mtime {
            let _ = self.set_modified_time(dst, mt).await;
        }
        Ok(())
    }
    async fn copy_with_progress(
        &self,
        src: &std::path::Path,
        dst: &std::path::Path,
        id: usize,
        tx: &tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
        cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<()> {
        let metadata = tokio::fs::metadata(src).await?;
        let total_size = metadata.len();
        let mtime = metadata.modified().ok();

        let mut reader = tokio::fs::File::open(src).await?;
        let mut writer = tokio::fs::File::create(dst).await?;

        let mut buffer = vec![0; 64 * 1024]; // 64KB chunks
        let mut processed = 0;

        while processed < total_size {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(());
            }

            let n = reader.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            writer.write_all(&buffer[..n]).await?;
            processed += n as u64;

            let _ = tx.send(crate::tasks::TaskEvent::UpdateByteProgress(
                id, processed, total_size,
            ));
        }

        if let Some(mt) = mtime {
            let _ = self.set_modified_time(dst, mt).await;
        }

        Ok(())
    }
    async fn get_size(&self, path: &std::path::Path) -> anyhow::Result<u64> {
        Ok(tokio::fs::metadata(path).await?.len())
    }
    async fn read_file(&self, path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
        Ok(tokio::fs::read(path).await?)
    }
    async fn read_chunk(
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
    async fn write_chunk(
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

    async fn get_permissions(&self, _path: &std::path::Path) -> Option<u32> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::metadata(_path)
                .await
                .ok()
                .map(|m| m.permissions().mode() & 0o777)
        }
        #[cfg(not(unix))]
        {
            let _ = _path;
            None
        }
    }

    async fn set_permissions(&self, path: &std::path::Path, mode: u32) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = tokio::fs::metadata(path).await?;
            let mut perms = metadata.permissions();
            perms.set_mode(mode);
            tokio::fs::set_permissions(path, perms).await?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            let _ = mode;
            Err(anyhow::anyhow!(
                "Permissions not supported on this platform"
            ))
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
    ) -> anyhow::Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let duration = mtime
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| std::io::Error::other("invalid mtime"))?;
            #[cfg(unix)]
            {
                let _sec = duration.as_secs() as libc::time_t;
                let _nsec = duration.subsec_nanos() as libc::c_long;
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
        .await??;
        Ok(())
    }

    async fn write_file_with_permissions(
        &self,
        path: &std::path::Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> anyhow::Result<()> {
        tokio::fs::write(path, data).await?;
        if let Some(_mode_val) = mode {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = tokio::fs::metadata(path).await {
                    let current_mode = metadata.permissions().mode();
                    let new_mode = (current_mode & !0o777) | (_mode_val & 0o777);
                    let mut perms = metadata.permissions();
                    perms.set_mode(new_mode);
                    tokio::fs::set_permissions(path, perms).await?;
                }
            }
        }
        Ok(())
    }

    fn context_key(&self) -> String {
        "std_local".to_string()
    }

    fn is_local(&self) -> bool {
        true
    }
}
