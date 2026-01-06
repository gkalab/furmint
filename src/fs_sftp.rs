use crate::fs_ops::FileEntry;
use crate::fs_provider::FileSystemProvider;
use anyhow::{Result, anyhow};
use ssh2::Session;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct SftpFs {
    session: Mutex<Session>,
    _host: String,
    _user: String,
    prefix: String,
}

impl SftpFs {
    pub fn new(session: Session, host: String, user: String) -> Self {
        let prefix = format!("[{}@{}]", user, host);
        // Enable keep-alive every 10 seconds
        session.set_keepalive(true, 10);
        Self {
            session: Mutex::new(session),
            _host: host,
            _user: user,
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
            .map_err(|e| anyhow!("Failed to open SFTP channel: {}", e))?;
        f(&sftp)
    }
}

impl FileSystemProvider for SftpFs {
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        self.with_sftp(|sftp| {
            let entries = match sftp.readdir(path) {
                Ok(e) => e,
                Err(_) => {
                    // Try to canonicalize and read again (handle symlinks or weird paths)
                    let real_path = sftp.realpath(path).map_err(|e| {
                        anyhow!("Failed to read directory (and realpath failed): {}", e)
                    })?;
                    sftp.readdir(&real_path)
                        .map_err(|e| anyhow!("Failed to read directory: {}", e))?
                }
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

                let is_dir = stat.is_dir();
                let is_symlink = stat.file_type().is_symlink();
                let size = if is_dir { None } else { stat.size };
                let modified = stat
                    .mtime
                    .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(t as u64));

                result.push(FileEntry {
                    name,
                    is_dir,
                    is_symlink,
                    size,
                    modified,
                    attributes: format_permissions(stat.perm),
                    selected: false,
                });
            }

            Ok(result)
        })
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        self.with_sftp(|sftp| {
            sftp.mkdir(path, 0o755)
                .map_err(|e| anyhow!("Failed to create directory: {}", e))
        })
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        self.with_sftp(|sftp| {
            sftp.create(path)
                .map_err(|e| anyhow!("Failed to create file: {}", e))?;
            Ok(())
        })
    }

    fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        self.with_sftp(|sftp| {
            let stat = sftp
                .stat(path)
                .map_err(|e| anyhow!("Failed to stat path: {}", e))?;

            if stat.is_dir() {
                if recursive {
                    // SFTP doesn't have recursive delete, we must do it manually
                    Self::delete_recursive_internal(sftp, path)?;
                } else {
                    sftp.rmdir(path)
                        .map_err(|e| anyhow!("Failed to remove directory: {}", e))?;
                }
            } else {
                sftp.unlink(path)
                    .map_err(|e| anyhow!("Failed to remove file: {}", e))?;
            }
            Ok(())
        })
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.with_sftp(|sftp| {
            sftp.rename(from, to, None)
                .map_err(|e| anyhow!("Failed to rename: {}", e))
        })
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.with_sftp(|sftp| {
            use std::io::Read;
            let mut file = sftp
                .open(path)
                .map_err(|e| anyhow!("Failed to open file: {}", e))?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .map_err(|e| anyhow!("Failed to read file: {}", e))?;
            Ok(buffer)
        })
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        self.with_sftp(|sftp| {
            use std::io::Write;
            let mut file = sftp
                .create(path)
                .map_err(|e| anyhow!("Failed to create file: {}", e))?;
            file.write_all(data)
                .map_err(|e| anyhow!("Failed to write file: {}", e))?;
            Ok(())
        })
    }

    fn display_prefix(&self) -> &str {
        &self.prefix
    }

    fn is_local(&self) -> bool {
        false
    }

    fn exists(&self, path: &Path) -> bool {
        self.with_sftp(|sftp| Ok(sftp.stat(path).is_ok()))
            .unwrap_or(false)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.with_sftp(|sftp| Ok(sftp.stat(path).map(|s| s.is_dir()).unwrap_or(false)))
            .unwrap_or(false)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        self.with_sftp(|sftp| {
            let path = sftp
                .realpath(path)
                .map_err(|e| anyhow!("Failed to canonicalize path: {}", e))?;
            Ok(path)
        })
    }

    fn context_key(&self) -> String {
        // prefix is formatted as "[user@host]"
        self.prefix.clone()
    }
}

impl SftpFs {
    fn delete_recursive_internal(sftp: &ssh2::Sftp, path: &Path) -> Result<()> {
        let entries = sftp.readdir(path)?;
        for (entry_path, stat) in entries {
            let file_name = entry_path.file_name().and_then(|n| n.to_str());
            if file_name == Some(".") || file_name == Some("..") {
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

fn format_permissions(perm: Option<u32>) -> String {
    let perm = perm.unwrap_or(0);
    let mut s = String::with_capacity(10);

    // Type (simplified, we mostly care about dir/file/link which are handled in entry)
    s.push(if perm & 0o040_000 != 0 {
        'd'
    } else if perm & 0o120_000 != 0 {
        'l'
    } else {
        '-'
    });

    // User
    s.push(if perm & 0o400 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o200 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o100 != 0 { 'x' } else { '-' });

    // Group
    s.push(if perm & 0o040 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o020 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o010 != 0 { 'x' } else { '-' });

    // Other
    s.push(if perm & 0o004 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o002 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o001 != 0 { 'x' } else { '-' });

    s
}
