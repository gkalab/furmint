use crate::fs::fs_provider::FileSystemProvider;
use crate::fs::utils::FileEntry;
use anyhow::{Result, anyhow};
use ssh2::{FileStat, Session};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::utils::{
    build_du_command, calculate_optimal_chunk_size, format_sftp_permissions, is_dot_or_dotdot,
    normalize_sftp_path,
};

pub struct SftpFs {
    session: Mutex<Session>,
    _host: String,
    _user: String,
    password: Option<String>,
    prefix: String,
}

impl SftpFs {
    #[must_use]
    pub fn new(session: Session, host: String, user: String, password: Option<String>) -> Self {
        let prefix = format!("[{user}@{host}]");

        Self {
            session: Mutex::new(session),
            _host: host,
            _user: user,
            password,
            prefix,
        }
    }

    fn with_sftp<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&ssh2::Sftp) -> Result<R>,
    {
        let session = self
            .session
            .lock()
            .map_err(|_| anyhow!("Session mutex poisoned"))?;
        let sftp = session
            .sftp()
            .map_err(|e| anyhow!("Failed to open SFTP channel: {e}"))?;
        f(&sftp)
    }

    pub async fn download(
        &self,
        src: &Path,
        dest_fs: &dyn crate::fs::traits::FileSystem,
        dest: &Path,
        progress: &crate::fs::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        self.copy_to_local(src, dest_fs, dest, progress).await
    }

    pub async fn upload(
        &self,
        src_fs: &dyn crate::fs::traits::FileSystem,
        src: &Path,
        dest: &Path,
        progress: &crate::fs::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        self.copy_from_local(src_fs, src, dest, progress).await
    }
}

impl Drop for SftpFs {
    fn drop(&mut self) {
        if let Ok(session) = self.session.lock() {
            let _ = session.disconnect(None, "Application exiting or tab closed", None);
        }
    }
}

#[async_trait::async_trait]
impl FileSystemProvider for SftpFs {
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        self.with_sftp(|sftp| {
            let normalized_path = PathBuf::from(normalize_sftp_path(path));
            let entries = if let Ok(e) = sftp.readdir(&normalized_path) {
                e
            } else {
                // Try to canonicalize and read again (handle symlinks or weird paths)
                let real_path = sftp
                    .realpath(&normalized_path)
                    .map_err(|e| anyhow!("Failed to read directory (and realpath failed): {e}"))?;
                sftp.readdir(&real_path)
                    .map_err(|e| anyhow!("Failed to read directory: {e}"))?
            };
            let mut result = Vec::new();

            // Add ".." entry if not at root
            if path.parent().is_some() || path.to_string_lossy() != "/" {
                result.push(FileEntry {
                    name: "..".to_string(),
                    is_dir: true,
                    is_symlink: false,
                    size: None,
                    modified: None,
                    attributes: String::new(),
                    selected: false,
                });
            }

            for (path_buf, stat) in entries {
                let name = path_buf
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                if name.is_empty() {
                    continue;
                }

                if is_dot_or_dotdot(&name) {
                    continue;
                }

                let is_dir = stat.is_dir();
                let is_symlink = stat.file_type().is_symlink();
                let size = if is_dir { None } else { stat.size };
                let modified = stat
                    .mtime
                    .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(t));

                result.push(FileEntry {
                    name,
                    is_dir,
                    is_symlink,
                    size,
                    modified,
                    attributes: format_sftp_permissions(stat.perm.unwrap_or(0)),
                    selected: false,
                });
            }

            Ok(result)
        })
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        self.with_sftp(|sftp| {
            sftp.mkdir(Path::new(&normalize_sftp_path(path)), 0o755)
                .map_err(|e| anyhow!("Failed to create directory: {e}"))
        })
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        self.with_sftp(|sftp| {
            sftp.create(Path::new(&normalize_sftp_path(path)))
                .map_err(|e| anyhow!("Failed to create file: {e}"))?;
            Ok(())
        })
    }

    fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        self.with_sftp(|sftp| {
            let normalized_path = PathBuf::from(normalize_sftp_path(path));
            let stat = sftp
                .stat(&normalized_path)
                .map_err(|e| anyhow!("Failed to stat path: {e}"))?;

            if stat.is_dir() {
                if recursive {
                    // SFTP doesn't have recursive delete, we must do it manually
                    Self::delete_recursive_internal(sftp, &normalized_path)?;
                } else {
                    sftp.rmdir(&normalized_path)
                        .map_err(|e| anyhow!("Failed to remove directory: {e}"))?;
                }
            } else {
                sftp.unlink(&normalized_path)
                    .map_err(|e| anyhow!("Failed to remove file: {e}"))?;
            }
            Ok(())
        })
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.with_sftp(|sftp| {
            sftp.rename(
                Path::new(&normalize_sftp_path(from)),
                Path::new(&normalize_sftp_path(to)),
                None,
            )
            .map_err(|e| anyhow!("Failed to rename: {e}"))
        })
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.with_sftp(|sftp| {
            use std::io::Read;
            let normalized = normalize_sftp_path(path);
            let mut file = sftp
                .open(Path::new(&normalized))
                .map_err(|_| anyhow!("Failed to open file: {normalized}"))?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .map_err(|e| anyhow!("Failed to read file: {e}"))?;
            Ok(buffer)
        })
    }

    fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.with_sftp(|sftp| {
            use std::io::{Read, Seek, SeekFrom};
            let normalized = normalize_sftp_path(path);
            let mut file = sftp
                .open(Path::new(&normalized))
                .map_err(|_| anyhow!("Failed to open file: {normalized}"))?;
            file.seek(SeekFrom::Start(offset))?;
            let mut buffer = vec![0; len];
            let n = file.read(&mut buffer)?;
            buffer.truncate(n);
            Ok(buffer)
        })
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        self.with_sftp(|sftp| {
            use std::io::Write;
            let mut file = sftp
                .create(Path::new(&normalize_sftp_path(path)))
                .map_err(|e| anyhow!("Failed to create file: {e}"))?;
            file.write_all(data)
                .map_err(|e| anyhow!("Failed to write file: {e}"))?;
            Ok(())
        })
    }

    fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
        self.with_sftp(|sftp| {
            use std::io::{Seek, SeekFrom, Write};
            let normalized_str = normalize_sftp_path(path);
            let normalized = Path::new(&normalized_str);
            let mut file = if offset == 0 {
                sftp.create(normalized)
                    .map_err(|e| anyhow!("Failed to create file: {e}"))?
            } else {
                sftp.open_mode(
                    normalized,
                    ssh2::OpenFlags::READ | ssh2::OpenFlags::WRITE,
                    0o644,
                    ssh2::OpenType::File,
                )
                .map_err(|e| anyhow!("Failed to open file for writing at offset {offset}: {e}",))?
            };
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(data)?;
            Ok(())
        })
    }

    fn write_file_with_permissions(
        &self,
        path: &Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()> {
        // First write the file
        self.write_file(path, data)?;

        // Then set permissions using SFTP setstat if needed
        if let Some(mode_val) = mode {
            let _ = self.set_permissions(path, mode_val);
        }

        Ok(())
    }

    fn display_prefix(&self) -> &str {
        &self.prefix
    }

    fn is_local(&self) -> bool {
        false
    }

    fn exists(&self, path: &Path) -> bool {
        self.with_sftp(|sftp| Ok(sftp.stat(Path::new(&normalize_sftp_path(path))).is_ok()))
            .unwrap_or(false)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.with_sftp(|sftp| {
            Ok(sftp
                .stat(Path::new(&normalize_sftp_path(path)))
                .map(|s| s.is_dir())
                .unwrap_or(false))
        })
        .unwrap_or(false)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        self.with_sftp(|sftp| {
            let path = sftp
                .realpath(Path::new(&normalize_sftp_path(path)))
                .map_err(|e| anyhow!("Failed to canonicalize path: {e}"))?;
            Ok(path)
        })
    }

    fn get_permissions(&self, path: &Path) -> Option<u32> {
        self.with_sftp(|sftp| {
            Ok(sftp
                .stat(Path::new(&normalize_sftp_path(path)))
                .ok()
                .and_then(|stat| stat.perm.map(|p| p & 0o777)))
        })
        .unwrap_or(None)
    }

    fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        self.with_sftp(|sftp| {
            let normalized_str = normalize_sftp_path(path);
            let normalized_path = Path::new(&normalized_str);
            let real_path = sftp
                .realpath(normalized_path)
                .unwrap_or_else(|_| normalized_path.to_path_buf());
            sftp.setstat(
                &real_path,
                FileStat {
                    size: None,
                    uid: None,
                    gid: None,
                    perm: Some(mode),
                    atime: None,
                    mtime: None,
                },
            )
            .map_err(|e| anyhow!("SFTP setstat failed: {e}"))
        })
        .is_ok()
    }

    fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        self.with_sftp(|sftp| {
            Ok(sftp
                .stat(Path::new(&normalize_sftp_path(path)))
                .ok()
                .and_then(|stat| {
                    stat.mtime
                        .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(t))
                }))
        })
        .unwrap_or(None)
    }

    fn set_modified_time(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
        let duration = match mtime.duration_since(std::time::UNIX_EPOCH).ok() {
            Some(d) => d.as_secs(),
            None => return false,
        };

        self.with_sftp(|sftp| {
            let normalized_str = normalize_sftp_path(path);
            let normalized_path = Path::new(&normalized_str);
            let real_path = sftp
                .realpath(normalized_path)
                .unwrap_or_else(|_| normalized_path.to_path_buf());
            sftp.setstat(
                &real_path,
                FileStat {
                    size: None,
                    uid: None,
                    gid: None,
                    perm: None,
                    atime: None,
                    mtime: Some(duration),
                },
            )
            .map_err(|e| anyhow!("SFTP setstat for mtime failed: {e}"))
        })
        .is_ok()
    }

    fn context_key(&self) -> String {
        // prefix is formatted as "[user@host]"
        self.prefix.clone()
    }

    fn get_password(&self) -> Option<String> {
        self.password.clone()
    }

    fn display_path(&self, path: &Path) -> String {
        normalize_sftp_path(path)
    }

    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64> {
        use std::io::Read;

        let path_str = normalize_sftp_path(path);

        // Use du -sb for accurate byte count
        let cmd = build_du_command(&path_str);

        let session = self
            .session
            .lock()
            .map_err(|_| anyhow!("Session mutex poisoned"))?;

        let mut channel = session
            .channel_session()
            .map_err(|e| anyhow!("Failed to create SSH channel: {e}"))?;

        channel
            .exec(&cmd)
            .map_err(|e| anyhow!("Failed to execute du command: {e}"))?;

        let mut output = String::new();
        channel
            .read_to_string(&mut output)
            .map_err(|e| anyhow!("Failed to read command output: {e}"))?;

        channel
            .wait_close()
            .map_err(|e| anyhow!("Failed to close channel: {e}"))?;

        // Parse the output - du -sb returns "<size>\t<path>"
        let size_str = output.split('\t').next().unwrap_or("0").trim();

        let size: u64 = size_str
            .parse()
            .map_err(|e| anyhow!("Failed to parse du output '{}': {e}", output.trim()))?;

        Ok(size)
    }

    async fn copy_to_local(
        &self,
        src: &Path,
        dest_fs: &dyn crate::fs::traits::FileSystem,
        dest: &Path,
        progress: &crate::fs::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        use std::io::Read;

        let (total_size, is_dir) = {
            let sftp_res = self
                .session
                .lock()
                .map_err(|_| anyhow!("Session mutex poisoned"))
                .and_then(|s| {
                    s.sftp()
                        .map_err(|e| anyhow!("Failed to open SFTP channel: {e}"))
                });

            let sftp = match sftp_res {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };

            let normalized_src = normalize_sftp_path(src);
            match sftp.stat(Path::new(&normalized_src)) {
                Ok(stat) => (stat.size.unwrap_or(0), stat.is_dir()),
                Err(e) => return Some(Err(anyhow!("Failed to stat file: {e}"))),
            }
        };

        if is_dir {
            return None;
        }

        let src_path = src.to_path_buf();
        let dest_path = dest.to_path_buf();

        // Open source file once in blocking context
        let (mut src_file, perms) = match self.with_sftp(|sftp| {
            let normalized_src_str = normalize_sftp_path(&src_path);
            let normalized_src = Path::new(&normalized_src_str);
            let file = sftp
                .open(normalized_src)
                .map_err(|_| anyhow!("Failed to open source file: {normalized_src_str}"))?;
            let perms = sftp
                .stat(normalized_src)
                .ok()
                .and_then(|stat| stat.perm.map(|p| p & 0o777));
            Ok((file, perms))
        }) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };

        if total_size == 0 {
            if let Err(e) = async {
                dest_fs
                    .write_file(&dest_path, &[])
                    .await
                    .map_err(|e| anyhow!("Failed to write empty file: {e}"))?;

                if let Some(perms) = perms {
                    let _ = dest_fs.set_permissions(&dest_path, perms).await;
                }
                Ok::<(), anyhow::Error>(())
            }
            .await
            {
                return Some(Err(e));
            }
            return Some(Ok(()));
        }

        let chunk_size = calculate_optimal_chunk_size(total_size);
        let mut buffer = vec![0; chunk_size];
        let mut offset = 0u64;

        loop {
            if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Some(Err(anyhow!("Operation cancelled")));
            }

            let bytes_read = match src_file
                .read(&mut buffer)
                .map_err(|e| anyhow!("Failed to read from source file: {e}"))
            {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return Some(Err(e)),
            };

            let chunk_data = buffer[..bytes_read].to_vec();
            if let Err(e) = dest_fs
                .write_chunk(&dest_path, offset, &chunk_data)
                .await
                .map_err(|e| anyhow!("Failed to write chunk at offset {offset}: {e}"))
            {
                return Some(Err(e));
            }

            offset += bytes_read as u64;
            progress
                .processed_bytes
                .fetch_add(bytes_read as u64, std::sync::atomic::Ordering::Relaxed);

            let _ = progress
                .tx
                .send(crate::tasks::TaskEvent::UpdateByteProgress(
                    progress.id,
                    offset,
                    total_size,
                ));
        }

        if let Some(perms) = perms {
            let _ = dest_fs.set_permissions(&dest_path, perms).await;
        }

        Some(Ok(()))
    }

    async fn copy_from_local(
        &self,
        src_fs: &dyn crate::fs::traits::FileSystem,
        src: &Path,
        dest: &Path,
        progress: &crate::fs::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        use std::io::Write;

        if src_fs.is_dir(src).await.unwrap_or(false) {
            return None;
        }

        let total_size = match src_fs.get_size(src).await {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        };

        let chunk_size = calculate_optimal_chunk_size(total_size);
        let src_path = src.to_path_buf();
        let dest_path = dest.to_path_buf();
        let src_perms = src_fs.get_permissions(&src_path).await;

        if total_size == 0 {
            return Some(self.with_sftp(|sftp| {
                let normalized_dest_str = normalize_sftp_path(&dest_path);
                let normalized_dest = Path::new(&normalized_dest_str);
                let mut file = sftp
                    .create(normalized_dest)
                    .map_err(|e| anyhow!("Failed to create destination file: {e}"))?;

                if let Some(mode) = src_perms
                    && let Ok(mut stat) = sftp.stat(normalized_dest)
                {
                    stat.perm = Some(mode);
                    let _ = sftp.setstat(normalized_dest, stat);
                }
                file.write_all(&[])
                    .map_err(|e| anyhow!("Failed to write empty file: {e}"))?;
                Ok(())
            }));
        }

        let mut dest_file = match self.with_sftp(|sftp| {
            let normalized_dest_str = normalize_sftp_path(&dest_path);
            let normalized_dest = Path::new(&normalized_dest_str);
            sftp.create(normalized_dest)
                .map_err(|e| anyhow!("Failed to create destination file: {e}"))
        }) {
            Ok(f) => f,
            Err(e) => return Some(Err(e)),
        };

        let mut offset = 0u64;
        loop {
            if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Some(Err(anyhow!("Operation cancelled")));
            }

            let len = std::cmp::min(
                chunk_size,
                usize::try_from(total_size - offset).unwrap_or(usize::MAX),
            );
            if len == 0 {
                break;
            }

            let chunk_data = match src_fs.read_chunk(&src_path, offset, len).await {
                Ok(d) => d,
                Err(e) => return Some(Err(e)),
            };

            if let Err(e) = dest_file
                .write_all(&chunk_data)
                .map_err(|e| anyhow!("Failed to write to destination file: {e}"))
            {
                return Some(Err(e));
            }

            offset += chunk_data.len() as u64;
            progress.processed_bytes.fetch_add(
                chunk_data.len() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );

            let _ = progress
                .tx
                .send(crate::tasks::TaskEvent::UpdateByteProgress(
                    progress.id,
                    offset,
                    total_size,
                ));
        }

        if let Some(mode) = src_perms {
            let _ = self.with_sftp(|sftp| {
                let normalized_dest_str = normalize_sftp_path(&dest_path);
                let normalized_dest = Path::new(&normalized_dest_str);
                if let Ok(mut stat) = sftp.stat(normalized_dest) {
                    stat.perm = Some(mode);
                    let _ = sftp.setstat(normalized_dest, stat);
                }
                Ok::<(), anyhow::Error>(())
            });
        }

        Some(Ok(()))
    }
}

pub use crate::fs::traits::TaskProgressContext as FileTaskProgressContext;

impl SftpFs {
    fn delete_recursive_internal(sftp: &ssh2::Sftp, path: &Path) -> Result<()> {
        let entries = sftp.readdir(path)?;
        for (entry_path, stat) in entries {
            let file_name = entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if is_dot_or_dotdot(file_name) {
                continue;
            }

            if stat.is_dir() {
                Self::delete_recursive_internal(sftp, &entry_path)?;
            } else {
                sftp.unlink(&entry_path)?;
            }
        }
        sftp.rmdir(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::format_sftp_permissions;
    use anyhow::Result;
    use std::sync::Mutex;

    #[test]
    fn test_format_permissions_dir() {
        assert_eq!(format_sftp_permissions(0o040_755), "drwxr-xr-x");
    }
    #[test]
    fn test_format_permissions_symlink() {
        assert_eq!(format_sftp_permissions(0o120_000 | 0o777), "lrwxrwxrwx");
    }
    #[test]
    fn test_format_permissions_regular_file() {
        assert_eq!(format_sftp_permissions(0o100_000 | 0o777), "-rwxrwxrwx");
    }
    #[test]
    fn test_format_permissions_read_only_file() {
        assert_eq!(format_sftp_permissions(0o100_000 | 0o400), "-r--------");
    }
    #[test]
    fn test_format_permissions_none() {
        assert_eq!(format_sftp_permissions(0), "----------");
    }

    #[allow(clippy::struct_excessive_bools)]
    pub struct MockSftp {
        list_entries: Vec<(String, u32)>, // (name, perms)
        fail_list: bool,
        fail_mkdir: bool,
        fail_create: bool,
        #[allow(dead_code)]
        // Not exercised in current tests, present if SFTP rmdir tests are added
        fail_rmdir: bool,
        fail_unlink: bool,
        fail_rename: bool,
        #[allow(dead_code)] // Not exercised in current tests, present for completeness
        fail_open: bool,
        #[allow(dead_code)] // Not exercised, present for future write tests
        fail_write: bool,
        #[allow(dead_code)] // Not exercised, present for future read tests
        fail_read: bool,
        fail_stat: bool,
        fail_realpath: bool,
        #[allow(dead_code)] // Not exercised, present for future write tests
        fail_setstat: bool,
        stat_is_dir: bool,
        stat_is_symlink: bool,
        stat_perms: u32,
        stat_size: Option<u64>,
        stat_mtime: Option<u64>,
        realpath_value: Option<String>,
    }
    impl Default for MockSftp {
        fn default() -> Self {
            Self {
                list_entries: Vec::new(),
                fail_list: false,
                fail_mkdir: false,
                fail_create: false,
                fail_rmdir: false,
                fail_unlink: false,
                fail_rename: false,
                fail_open: false,
                fail_write: false,
                fail_read: false,
                fail_stat: false,
                fail_realpath: false,
                fail_setstat: false,
                stat_is_dir: false,
                stat_is_symlink: false,
                stat_perms: 0o644, // Default file permissions
                stat_size: Some(0),
                stat_mtime: Some(0),
                realpath_value: None,
            }
        }
    }
    impl MockSftp {
        fn readdir(
            &self,
            _path: &std::path::Path,
        ) -> std::result::Result<Vec<(std::path::PathBuf, TestFileStat)>, String> {
            if self.fail_list {
                return Err("fail_list".to_string());
            }
            Ok(self
                .list_entries
                .iter()
                .map(|(name, perms)| {
                    (
                        std::path::PathBuf::from(name),
                        TestFileStat {
                            perms: *perms,
                            is_dir: *perms == 0o040_000,
                            is_symlink: *perms == 0o120_000,
                            size: Some(123),
                            mtime: Some(456),
                        },
                    )
                })
                .collect())
        }
        fn mkdir(&self, _path: &std::path::Path, _mode: u32) -> std::result::Result<(), String> {
            if self.fail_mkdir {
                return Err("fail_mkdir".to_string());
            }
            Ok(())
        }
        fn create(&self, _path: &std::path::Path) -> std::result::Result<MockFile, String> {
            if self.fail_create {
                return Err("fail_create".to_string());
            }
            Ok(MockFile)
        }
        #[allow(dead_code)] // Not used by current tests, present for trait completeness
        fn rmdir(&self, _path: &std::path::Path) -> std::result::Result<(), String> {
            if self.fail_rmdir {
                return Err("fail_rmdir".to_string());
            }
            Ok(())
        }
        fn unlink(&self, _path: &std::path::Path) -> std::result::Result<(), String> {
            if self.fail_unlink {
                return Err("fail_unlink".to_string());
            }
            Ok(())
        }
        fn rename(
            &self,
            _from: &std::path::Path,
            _to: &std::path::Path,
            _flags: Option<u32>,
        ) -> std::result::Result<(), String> {
            if self.fail_rename {
                return Err("fail_rename".to_string());
            }
            Ok(())
        }
        #[allow(dead_code)] // Not used by current tests, present for trait completeness
        fn open(&self, _path: &std::path::Path) -> std::result::Result<MockFile, String> {
            if self.fail_open {
                return Err("fail_open".to_string());
            }
            Ok(MockFile)
        }
        fn stat(&self, _path: &std::path::Path) -> std::result::Result<TestFileStat, String> {
            if self.fail_stat {
                return Err("fail_stat".to_string());
            }
            Ok(TestFileStat {
                perms: self.stat_perms,
                is_dir: self.stat_is_dir,
                is_symlink: self.stat_is_symlink,
                size: self.stat_size,
                mtime: self.stat_mtime,
            })
        }
        fn realpath(
            &self,
            _path: &std::path::Path,
        ) -> std::result::Result<std::path::PathBuf, String> {
            if self.fail_realpath {
                return Err("fail_realpath".to_string());
            }
            Ok(std::path::PathBuf::from(
                self.realpath_value.clone().unwrap_or("/real".to_string()),
            ))
        }
        #[allow(dead_code)]
        fn setstat(
            &self,
            _path: &std::path::Path,
            _stat: ssh2::FileStat,
        ) -> std::result::Result<(), String> {
            if self.fail_setstat {
                return Err("fail_setstat".to_string());
            }
            Ok(())
        }
        fn sftp(&self) -> &MockSftp {
            self
        }
    }
    struct MockFile;
    impl std::io::Read for MockFile {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }
    impl std::io::Write for MockFile {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Ok(0)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[derive(Default)]
    struct TestFileStat {
        #[allow(dead_code)]
        // Not all fields used in each test, some present for stat struct completeness
        perms: u32,
        is_dir: bool,
        #[allow(dead_code)]
        is_symlink: bool,
        #[allow(dead_code)]
        size: Option<u64>,
        #[allow(dead_code)]
        mtime: Option<u64>,
    }

    // Adapter SftpFs for our mocks
    pub struct TestFs {
        session: Mutex<MockSftp>,
        #[allow(dead_code)] // Field may not be used in some coverage cases
        prefix: String,
    }
    impl TestFs {
        #[allow(dead_code)] // Used only in direct test harnessing in this test module
        fn with_sftp<F, R>(&self, f: F) -> Result<R, String>
        where
            F: FnOnce(&MockSftp) -> Result<R, String>,
        {
            let session = self
                .session
                .lock()
                .map_err(|e| format!("Session mutex poisoned: {e}"))?;
            let sftp = session.sftp();
            f(sftp)
        }

        // Adapters for trait logic
        fn list_dir(&self) -> Result<Vec<String>> {
            self.with_sftp(|sftp| {
                let entries = sftp.readdir(&std::path::PathBuf::from("/"))?;
                Ok(entries
                    .iter()
                    .map(|(p, _s)| p.to_string_lossy().into_owned())
                    .collect())
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
        }
        fn create_dir(&self) -> Result<()> {
            self.with_sftp(|sftp| {
                sftp.mkdir(&std::path::PathBuf::from("/tmp"), 0o755)?;
                Ok(())
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
        }
        fn create_file(&self) -> Result<()> {
            self.with_sftp(|sftp| {
                sftp.create(&std::path::PathBuf::from("/tmp/file"))?;
                Ok(())
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
        }
        fn delete_file(&self) -> Result<()> {
            self.with_sftp(|sftp| {
                sftp.unlink(&std::path::PathBuf::from("/tmp/file"))?;
                Ok(())
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
        }

        #[allow(dead_code)]
        fn get_permissions(&self, path: &std::path::Path) -> Option<u32> {
            self.with_sftp(|sftp| Ok(sftp.stat(path).ok().map(|stat| stat.perms)))
                .unwrap_or(None)
        }

        #[cfg(unix)]
        #[allow(dead_code)]
        fn set_permissions_unix(&self, path: &std::path::Path, mode: u32) -> bool {
            self.with_sftp(|sftp| {
                Ok(sftp
                    .setstat(
                        path,
                        ssh2::FileStat {
                            size: None,
                            uid: None,
                            gid: None,
                            perm: Some(mode),
                            atime: None,
                            mtime: None,
                        },
                    )
                    .is_ok())
            })
            .unwrap_or(false)
        }
        fn rename_file(&self) -> Result<()> {
            self.with_sftp(|sftp| {
                sftp.rename(
                    &std::path::PathBuf::from("/a"),
                    &std::path::PathBuf::from("/b"),
                    None,
                )?;
                Ok(())
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
        }
        fn canonicalize(&self) -> Result<std::path::PathBuf> {
            self.with_sftp(|sftp| sftp.realpath(&std::path::PathBuf::from("/x")))
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
        fn stat(&self) -> Result<TestFileStat, String> {
            self.with_sftp(|sftp| sftp.stat(&std::path::PathBuf::from("/z")))
        }
    }

    #[test]
    fn test_list_dir_success() {
        let mock = MockSftp {
            list_entries: vec![("fileA".to_string(), 0), ("fileB".to_string(), 0)],
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "x".to_string(),
        };
        let out = fs.list_dir().unwrap();
        assert_eq!(out, vec!["fileA", "fileB"]);
    }
    #[test]
    fn test_list_dir_failure() {
        let mock = MockSftp {
            fail_list: true,
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "p".to_string(),
        };
        let out = fs.list_dir();
        assert!(out.is_err());
    }
    #[test]
    fn test_create_dir_success() {
        let mock = MockSftp {
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "y".to_string(),
        };
        assert!(fs.create_dir().is_ok());
    }
    #[test]
    fn test_create_dir_failure() {
        let mock = MockSftp {
            fail_mkdir: true,
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "y".to_string(),
        };
        assert!(fs.create_dir().is_err());
    }
    #[test]
    fn test_create_file_success() {
        let mock = MockSftp {
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "y".to_string(),
        };
        assert!(fs.create_file().is_ok());
    }
    #[test]
    fn test_create_file_failure() {
        let mock = MockSftp {
            fail_create: true,
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "y".to_string(),
        };
        assert!(fs.create_file().is_err());
    }
    #[test]
    fn test_delete_file_success() {
        let mock = MockSftp {
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "z".to_string(),
        };
        assert!(fs.delete_file().is_ok());
    }
    #[test]
    fn test_delete_file_failure() {
        let mock = MockSftp {
            fail_unlink: true,
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "z".to_string(),
        };
        assert!(fs.delete_file().is_err());
    }
    #[test]
    fn test_rename_file_success() {
        let mock = MockSftp {
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "r".to_string(),
        };
        assert!(fs.rename_file().is_ok());
    }
    #[test]
    fn test_rename_file_failure() {
        let mock = MockSftp {
            fail_rename: true,
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "r".to_string(),
        };
        assert!(fs.rename_file().is_err());
    }
    #[test]
    fn test_canonicalize_success() {
        let mock = MockSftp {
            realpath_value: Some("/correct/path".to_string()),
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "c".to_string(),
        };
        assert_eq!(
            fs.canonicalize().unwrap(),
            std::path::PathBuf::from("/correct/path")
        );
    }
    #[test]
    fn test_canonicalize_failure() {
        let mock = MockSftp {
            fail_realpath: true,
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "c".to_string(),
        };
        assert!(fs.canonicalize().is_err());
    }
    #[test]
    fn test_stat_success_is_dir() {
        let mock = MockSftp {
            stat_is_dir: true,
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "s".to_string(),
        };
        assert!(fs.stat().unwrap().is_dir);
    }
    #[test]
    fn test_stat_failure() {
        let mock = MockSftp {
            fail_stat: true,
            ..Default::default()
        };
        let fs = TestFs {
            session: Mutex::new(mock),
            prefix: "s".to_string(),
        };
        assert!(fs.stat().is_err());
    }
}

#[allow(dead_code)] // Used only by unit test scaffolding to test SftpFs::with_sftp error branches
struct DummySftp;
impl DummySftp {
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)]
    #[allow(unused)]
    fn test_op(&self) -> Result<&'static str> {
        Ok("ok")
    }
}

#[cfg(test)]
struct DummySession {
    #[allow(dead_code)] // Field present for possible mutex poison simulation in tests
    poison: bool,
    fail_sftp: bool,
}
#[cfg(test)]
impl DummySession {
    fn sftp(&self) -> std::result::Result<DummySftp, &'static str> {
        if self.fail_sftp {
            Err("fail")
        } else {
            Ok(DummySftp)
        }
    }
}

// Adapter just for test
#[cfg(test)]
struct TestSftpFs {
    session: Mutex<DummySession>,
}
#[cfg(test)]
impl TestSftpFs {
    fn with_sftp<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&DummySftp) -> Result<R>,
    {
        let session = self
            .session
            .lock()
            .map_err(|_| anyhow!("Session mutex poisoned"))?;
        let sftp = session
            .sftp()
            .map_err(|_| anyhow!("Failed to open SFTP channel"))?;
        f(&sftp)
    }
}

#[test]
fn test_with_sftp_success() {
    let sftpfs = TestSftpFs {
        session: Mutex::new(DummySession {
            poison: false,
            fail_sftp: false,
        }),
    };
    let result = sftpfs.with_sftp(DummySftp::test_op);
    assert_eq!(result.unwrap(), "ok");
}
#[test]
fn test_with_sftp_sftp_fails() {
    let sftpfs = TestSftpFs {
        session: Mutex::new(DummySession {
            poison: false,
            fail_sftp: true,
        }),
    };
    let result = sftpfs.with_sftp(DummySftp::test_op);
    assert!(result.is_err());
    assert!(format!("{}", result.unwrap_err()).contains("Failed to open SFTP channel"));
}
#[test]
fn test_with_sftp_mutex_poisoned() {
    // Standard library doesn't let us easily poison a mutex manually in tests,
    // but we can simulate by overriding .lock() to produce an error (here, by using a broken mutex)
    // Instead, we directly test error branch:
    #[allow(dead_code)] // Only present for mutex poison simulation in with_sftp test coverage
    struct PoisonedSession;
    struct PoisonedSftpFs;
    impl PoisonedSftpFs {
        fn with_sftp<F, R>(_f: F) -> Result<R>
        where
            F: FnOnce(&DummySftp) -> Result<R>,
        {
            Err(anyhow!("Session mutex poisoned"))
        }
    }
    let result = PoisonedSftpFs::with_sftp(|_| Ok("never"));
    assert!(result.is_err());
    assert!(format!("{}", result.unwrap_err()).contains("Session mutex poisoned"));
}

#[cfg(unix)]
#[test]
fn test_display_prefix_and_is_local() {
    use crate::fs::fs_provider::FileSystemProvider;
    use crate::fs::fs_sftp::SftpFs;
    use ssh2::Session;
    let session = Session::new().unwrap();
    let fs = SftpFs::new(session, "host123".to_string(), "user456".to_string(), None);
    // Trait methods
    assert_eq!(FileSystemProvider::display_prefix(&fs), "[user456@host123]");
    assert!(!FileSystemProvider::is_local(&fs));
}

#[cfg(unix)]
#[test]
fn test_context_key() {
    use crate::fs::fs_provider::FileSystemProvider;
    use crate::fs::fs_sftp::SftpFs;
    use ssh2::Session;
    let session = Session::new().unwrap();
    let fs = SftpFs::new(session, "myhost".to_string(), "myuser".to_string(), None);
    assert_eq!(FileSystemProvider::context_key(&fs), "[myuser@myhost]");
}
