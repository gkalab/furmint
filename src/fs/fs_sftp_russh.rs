//! Windows-only SFTP filesystem backend using russh + russh-sftp.
//!
//! On Linux the existing `fs_sftp` module backed by `ssh2`/`libssh2` is used.
//! On Windows `libssh2` has KEX negotiation failures against modern OpenSSH
//! servers, so this pure-Rust implementation is used instead.

use crate::fs::fs_provider::FileSystemProvider;
use crate::fs::utils::FileEntry;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use russh::client;
use russh::keys::ssh_key;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub(crate) struct SshClientHandler;

impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _key: &ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(true)
    }
}

pub struct SftpFs {
    session: Arc<client::Handle<SshClientHandler>>,
    sftp: Arc<SftpSession>,
    _host: String,
    _user: String,
    password: Option<String>,
    prefix: String,
}

impl SftpFs {
    /// Connect with password authentication and return a ready `SftpFs`.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH connection fails or authentication is rejected.
    pub async fn connect_password(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
    ) -> Result<Self> {
        let config = Arc::new(client::Config::default());
        let mut handle = client::connect(config, (host, port), SshClientHandler)
            .await
            .map_err(|e| anyhow!("SSH connect failed: {e}"))?;

        let ok = handle
            .authenticate_password(user, password)
            .await
            .map_err(|e| anyhow!("Password authentication error: {e}"))?;

        if !matches!(ok, russh::client::AuthResult::Success) {
            return Err(anyhow!("Password authentication rejected by server"));
        }

        Self::from_handle(
            handle,
            host.to_string(),
            user.to_string(),
            Some(password.to_string()),
        )
        .await
    }

    /// Connect using public-key authentication (tries default key files in ~/.ssh/).
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH connection fails or all found keys are rejected.
    pub async fn connect_pubkey(host: &str, port: u16, user: &str) -> Result<Self> {
        let config = Arc::new(client::Config::default());
        let mut handle = client::connect(config, (host, port), SshClientHandler)
            .await
            .map_err(|e| anyhow!("SSH connect failed: {e}"))?;

        let keys = find_default_ssh_keys();
        if keys.is_empty() {
            return Err(anyhow!("No SSH key files found in ~/.ssh/"));
        }

        let mut last_err = anyhow!("All key authentication attempts failed");
        for key_path in &keys {
            // Load the key via russh::keys::load_secret_key (re-exported or moved in 0.59)
            let key_pair = match russh::keys::load_secret_key(key_path, None) {
                Ok(k) => k,
                Err(e) => {
                    last_err = anyhow!("Failed to load {}: {e}", key_path.display());
                    continue;
                }
            };

            let pk = russh::keys::key::PrivateKeyWithHashAlg::new(
                Arc::new(key_pair),
                None, // default hash alg
            );

            match handle.authenticate_publickey(user, pk).await {
                Ok(russh::client::AuthResult::Success) => {
                    return Self::from_handle(handle, host.to_string(), user.to_string(), None)
                        .await;
                }
                Ok(_) => {
                    last_err = anyhow!("Key rejected by server: {}", key_path.display());
                }
                Err(e) => {
                    last_err = anyhow!("Key auth error for {}: {e}", key_path.display());
                }
            }
        }
        Err(last_err)
    }

    /// Open the SFTP subsystem over a freshly-authenticated session handle.
    async fn from_handle(
        handle: client::Handle<SshClientHandler>,
        host: String,
        user: String,
        password: Option<String>,
    ) -> Result<Self> {
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| anyhow!("Failed to open SSH channel: {e}"))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| anyhow!("Failed to request SFTP subsystem: {e}"))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| anyhow!("Failed to create SFTP session: {e}"))?;

        let prefix = format!("[{user}@{host}]");
        Ok(Self {
            session: Arc::new(handle),
            sftp: Arc::new(sftp),
            _host: host,
            _user: user,
            password,
            prefix,
        })
    }

    fn normalize_path(path: &Path) -> String {
        let mut s = path.to_string_lossy().replace('\\', "/");
        if !s.starts_with('/') {
            s = format!("/{s}");
        }
        while s.contains("//") {
            s = s.replace("//", "/");
        }
        s
    }

    fn calculate_optimal_chunk_size(file_size: u64) -> usize {
        match file_size {
            0..=512_000 => 512 * 1024,
            512_001..=8_000_000 => 2 * 1024 * 1024,
            8_000_001..=200_000_000 => 8 * 1024 * 1024,
            _ => 16 * 1024 * 1024,
        }
    }

    /// Run an async SFTP closure from a synchronous context using
    /// `block_in_place` + the current Tokio handle.
    fn run_async<F, Fut, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(Arc<SftpSession>) -> Fut,
        Fut: std::future::Future<Output = Result<R>>,
    {
        let sftp = Arc::clone(&self.sftp);
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f(sftp)))
    }

    /// Recursively delete a remote directory via SFTP.
    async fn delete_recursive(sftp: &SftpSession, path: &str) -> Result<()> {
        let entries = sftp
            .read_dir(path)
            .await
            .map_err(|e| anyhow!("readdir {path}: {e}"))?;
        for entry in entries {
            let name = entry.file_name().clone();
            if name == "." || name == ".." {
                continue;
            }
            let child = format!("{}/{}", path.trim_end_matches('/'), name);
            if entry.metadata().is_dir() {
                Box::pin(Self::delete_recursive(sftp, &child)).await?;
            } else {
                sftp.remove_file(child)
                    .await
                    .map_err(|e| anyhow!("unlink: {e}"))?;
            }
        }
        sftp.remove_dir(path)
            .await
            .map_err(|e| anyhow!("rmdir {path}: {e}"))
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
        let sftp = Arc::clone(&self.sftp);
        let session = Arc::clone(&self.session);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| {
                handle.block_on(async {
                    let _ = sftp.close().await;
                    let _ = session
                        .disconnect(russh::Disconnect::ByApplication, "", "en")
                        .await;
                });
            });
        }
    }
}

#[async_trait]
impl FileSystemProvider for SftpFs {
    // ---- directory listing ------------------------------------------------

    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        self.run_async(|sftp| async move {
            let path_str = SftpFs::normalize_path(path);
            let entries = sftp
                .read_dir(path_str)
                .await
                .map_err(|e| anyhow!("Failed to read directory: {e}"))?;

            let mut result = Vec::new();
            // prepend ".." for parent navigation
            result.push(FileEntry {
                name: "..".to_string(),
                is_dir: true,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: String::new(),
                selected: false,
            });

            for entry in entries {
                let name = entry.file_name().clone();
                if name.is_empty() || name == "." || name == ".." {
                    continue;
                }
                let meta = entry.metadata();
                let is_dir = meta.is_dir();
                let is_symlink = meta
                    .permissions
                    .is_some_and(|p| (p & 0o170_000) == 0o120_000);
                let size = if is_dir { None } else { meta.size };
                let modified = meta
                    .mtime
                    .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(t)));
                result.push(FileEntry {
                    name,
                    is_dir,
                    is_symlink,
                    size,
                    modified,
                    attributes: format_permissions(meta.permissions),
                    selected: false,
                });
            }
            Ok(result)
        })
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        self.run_async(|sftp| async move {
            let p = SftpFs::normalize_path(path);
            sftp.create_dir(p)
                .await
                .map_err(|e| anyhow!("Failed to create directory: {e}"))
        })
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        self.run_async(|sftp| async move {
            let p = SftpFs::normalize_path(path);
            sftp.create(p)
                .await
                .map_err(|e| anyhow!("Failed to create file: {e}"))?;
            Ok(())
        })
    }

    fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        self.run_async(|sftp| async move {
            let p = SftpFs::normalize_path(path);
            let meta = sftp
                .metadata(p.as_str())
                .await
                .map_err(|e| anyhow!("Failed to stat path: {e}"))?;
            if meta.is_dir() {
                if recursive {
                    SftpFs::delete_recursive(&sftp, &p).await
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
        })
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.run_async(|sftp| async move {
            sftp.rename(SftpFs::normalize_path(from), SftpFs::normalize_path(to))
                .await
                .map_err(|e| anyhow!("Failed to rename: {e}"))
        })
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.run_async(|sftp| async move {
            sftp.read(SftpFs::normalize_path(path))
                .await
                .map_err(|e| anyhow!("Failed to read file: {e}"))
        })
    }

    fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.run_async(|sftp| async move {
            let p = SftpFs::normalize_path(path);
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
        })
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        self.run_async(|sftp| async move {
            sftp.write(SftpFs::normalize_path(path), &data)
                .await
                .map_err(|e| anyhow!("Failed to write file: {e}"))
        })
    }

    fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        self.run_async(|sftp| async move {
            let p = SftpFs::normalize_path(path);
            if offset == 0 {
                sftp.write(p, &data)
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
                file.write_all(&data)
                    .await
                    .map_err(|e| anyhow!("Failed to write at offset {offset}: {e}"))?;
                Ok(())
            }
        })
    }

    fn write_file_with_permissions(
        &self,
        path: &Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> Result<()> {
        self.write_file(path, data)?;
        if let Some(m) = mode {
            let _ = self.set_permissions(path, m);
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
        self.run_async(|sftp| async move {
            sftp.try_exists(SftpFs::normalize_path(path))
                .await
                .map_err(|e| anyhow!("{e}"))
        })
        .unwrap_or(false)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.run_async(|sftp| async move {
            let meta = sftp
                .metadata(SftpFs::normalize_path(path))
                .await
                .map_err(|e| anyhow!("{e}"))?;
            Ok(meta.is_dir())
        })
        .unwrap_or(false)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        self.run_async(|sftp| async move {
            let s = sftp
                .canonicalize(SftpFs::normalize_path(path))
                .await
                .map_err(|e| anyhow!("Failed to canonicalize: {e}"))?;
            Ok(PathBuf::from(s))
        })
    }

    fn get_permissions(&self, path: &Path) -> Option<u32> {
        self.run_async(|sftp| async move {
            let meta = sftp
                .metadata(SftpFs::normalize_path(path))
                .await
                .map_err(|e| anyhow!("{e}"))?;
            Ok(meta.permissions.map(|p| p & 0o777))
        })
        .ok()
        .flatten()
    }

    fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        self.run_async(|sftp| async move {
            let p = SftpFs::normalize_path(path);
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
            .map_err(|e| anyhow!("setstat failed: {e}"))
        })
        .is_ok()
    }

    fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        self.run_async(|sftp| async move {
            let meta = sftp
                .metadata(SftpFs::normalize_path(path))
                .await
                .map_err(|e| anyhow!("{e}"))?;
            Ok(meta
                .mtime
                .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(t))))
        })
        .ok()
        .flatten()
    }

    fn set_modified_time(&self, path: &Path, mtime: std::time::SystemTime) -> bool {
        let secs = mtime
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
            .unwrap_or(0);
        self.run_async(|sftp| async move {
            sftp.set_metadata(
                SftpFs::normalize_path(path),
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
            .map_err(|e| anyhow!("setstat mtime failed: {e}"))
        })
        .is_ok()
    }

    fn context_key(&self) -> String {
        self.prefix.clone()
    }

    fn get_password(&self) -> Option<String> {
        self.password.clone()
    }

    fn display_path(&self, path: &Path) -> String {
        let mut s = path.to_string_lossy().replace('\\', "/");
        if !s.starts_with('/') {
            s = format!("/{s}");
        }
        while s.contains("//") {
            s = s.replace("//", "/");
        }
        s
    }

    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64> {
        let path_str = Self::normalize_path(path);
        let cmd = format!(
            "du -sb '{}' 2>/dev/null || echo 0",
            shell_escape::escape(std::borrow::Cow::Borrowed(&path_str))
        );

        let mut channel = self
            .session
            .channel_open_session()
            .await
            .map_err(|e| anyhow!("Failed to open channel: {e}"))?;

        channel
            .exec(true, cmd)
            .await
            .map_err(|e| anyhow!("Failed to exec du: {e}"))?;

        let mut output = Vec::new();
        loop {
            match channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    output.extend_from_slice(&data);
                }
                Some(russh::ChannelMsg::ExitStatus { .. } | russh::ChannelMsg::Eof) | None => break,
                _ => {}
            }
        }

        let out = String::from_utf8_lossy(&output);
        let size_str = out.split('\t').next().unwrap_or("0").trim();
        let size: u64 = size_str
            .parse()
            .map_err(|e| anyhow!("Failed to parse du output '{}': {e}", out.trim()))?;
        Ok(size)
    }

    async fn copy_to_local(
        &self,
        src: &Path,
        dest_fs: &dyn crate::fs::traits::FileSystem,
        dest: &Path,
        progress: &crate::fs::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        let src_str = Self::normalize_path(src);
        let dest_path = dest.to_path_buf();

        let (total_size, perms) = match self.sftp.metadata(src_str.as_str()).await {
            Ok(m) => (m.size.unwrap_or(0), m.permissions.map(|p| p & 0o777)),
            Err(e) => return Some(Err(anyhow!("Failed to stat: {e}"))),
        };

        if total_size == 0 {
            if let Err(e) = dest_fs.write_file(&dest_path, &[]).await {
                return Some(Err(anyhow!("{e}")));
            }
            if let Some(p) = perms {
                let _ = dest_fs.set_permissions(&dest_path, p).await;
            }
            return Some(Ok(()));
        }

        let mut src_file = match self.sftp.open(src_str).await {
            Ok(f) => f,
            Err(e) => return Some(Err(anyhow!("Failed to open source: {e}"))),
        };

        let chunk_size = Self::calculate_optimal_chunk_size(total_size);
        let mut buf = vec![0u8; chunk_size];
        let mut offset = 0u64;

        loop {
            if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Some(Err(anyhow!("Operation cancelled")));
            }
            let n = match src_file.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return Some(Err(anyhow!("Read error: {e}"))),
            };
            let chunk = buf[..n].to_vec();
            if let Err(e) = dest_fs.write_chunk(&dest_path, offset, &chunk).await {
                return Some(Err(anyhow!("{e}")));
            }
            offset += n as u64;
            progress
                .processed_bytes
                .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
            let _ = progress
                .tx
                .send(crate::tasks::TaskEvent::UpdateByteProgress(
                    progress.id,
                    offset,
                    total_size,
                ));
        }

        if let Some(p) = perms {
            let _ = dest_fs.set_permissions(&dest_path, p).await;
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
        let dest_str = Self::normalize_path(dest);
        let src_path = src.to_path_buf();

        let total_size = match src_fs.get_size(&src_path).await {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        };
        let src_perms = src_fs.get_permissions(&src_path).await;
        let chunk_size = Self::calculate_optimal_chunk_size(total_size);

        if total_size == 0 {
            return Some(
                self.sftp
                    .create(dest_str.as_str())
                    .await
                    .map(|_| ())
                    .map_err(|e| anyhow!("Failed to create empty file: {e}")),
            );
        }

        let mut dest_file = match self.sftp.create(dest_str.as_str()).await {
            Ok(f) => f,
            Err(e) => return Some(Err(anyhow!("Failed to create destination: {e}"))),
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
            let chunk = match src_fs.read_chunk(&src_path, offset, len).await {
                Ok(d) => d,
                Err(e) => return Some(Err(e)),
            };
            if let Err(e) = dest_file.write_all(&chunk).await {
                return Some(Err(anyhow!("Write error: {e}")));
            }
            offset += chunk.len() as u64;
            progress
                .processed_bytes
                .fetch_add(chunk.len() as u64, std::sync::atomic::Ordering::Relaxed);
            let _ = progress
                .tx
                .send(crate::tasks::TaskEvent::UpdateByteProgress(
                    progress.id,
                    offset,
                    total_size,
                ));
        }

        if let Some(mode) = src_perms {
            let _ = self
                .sftp
                .set_metadata(
                    dest_str,
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
                .await;
        }
        Some(Ok(()))
    }
}

fn find_default_ssh_keys() -> Vec<PathBuf> {
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("C:\\Users\\default"));
    let ssh_dir = home.join(".ssh");
    if !ssh_dir.is_dir() {
        return Vec::new();
    }
    // Search for common OpenSSH identity files.
    [
        "id_ed25519",
        "id_ecdsa",
        "id_rsa",
        "id_ed25519_sk",
        "id_ecdsa_sk",
        "id_rsa_sk",
    ]
    .iter()
    .filter(|&&f| ssh_dir.join(f).exists())
    .map(|f| ssh_dir.join(f))
    .collect()
}

fn format_permissions(perm: Option<u32>) -> String {
    let perm = perm.unwrap_or(0);
    let mut s = String::with_capacity(10);
    s.push(if (perm & 0o170_000) == 0o040_000 {
        'd'
    } else if (perm & 0o170_000) == 0o120_000 {
        'l'
    } else {
        '-'
    });
    s.push(if perm & 0o400 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o200 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o100 != 0 { 'x' } else { '-' });
    s.push(if perm & 0o040 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o020 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o010 != 0 { 'x' } else { '-' });
    s.push(if perm & 0o004 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o002 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o001 != 0 { 'x' } else { '-' });
    s
}
