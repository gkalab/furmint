//! SFTP filesystem backend using russh + russh-sftp.
//!
//! This pure-Rust implementation is used on all platforms. It replaces the
//! `ssh2`/`libssh2` backend, which had KEX negotiation failures against modern
//! OpenSSH servers. Public-key authentication tries the ssh-agent before
//! falling back to key files in `~/.ssh/`: on Unix via `SSH_AUTH_SOCK`, on
//! Windows via the OpenSSH named pipe (`\\.\pipe\openssh-ssh-agent`) with a
//! `PuTTY` Pageant fallback.

use crate::fs::fs_provider::FileSystemProvider;
use crate::fs::utils::FileEntry;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use russh::client;
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::ssh_key;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use secrecy::{ExposeSecret, SecretString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use super::utils::{
    build_du_command, calculate_optimal_chunk_size, find_default_ssh_keys, format_sftp_permissions,
    is_dot_or_dotdot, normalize_sftp_path,
};
use crate::ssh_known_hosts::HostKeyChecker;

/// Host-key verification failed. The server presented a key that is not in
/// `known_hosts` or differs from the stored one. This error is distinct
/// from authentication failures so callers can prompt the user with a TOFU
/// dialog instead of a password retry.
#[derive(Debug)]
pub struct HostKeyMismatch {
    pub presented_fp: String,
    pub stored_fp: Option<String>,
    pub key_line: String,
}

impl std::fmt::Display for HostKeyMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.stored_fp {
            Some(stored) => write!(
                f,
                "Host key changed (was {stored}, now {})",
                self.presented_fp
            ),
            None => write!(f, "Unknown host key {}", self.presented_fp),
        }
    }
}

impl std::error::Error for HostKeyMismatch {}

/// The SSH transport connection (TCP, KEX or transport handshake) failed,
/// before any authentication was attempted. Distinct from authentication
/// failures so callers can avoid offering a password prompt for a host that
/// was never reachable.
#[derive(Debug)]
pub struct SshConnectError {
    pub detail: String,
}

impl std::fmt::Display for SshConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for SshConnectError {}

/// Password authentication failed: either the server rejected the password or
/// the transport broke down while authenticating.
#[derive(Debug)]
pub struct PasswordAuthError {
    pub detail: String,
}

impl std::fmt::Display for PasswordAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for PasswordAuthError {}

/// Owns a freshly connected `client::Handle` until it is either handed over to
/// a `SftpFs` or the connect future fails/is cancelled. Dropping the guard
/// without handover sends a disconnect so the server session does not linger
/// until its own timeout.
struct ConnGuard {
    handle: Option<client::Handle<SshClientHandler>>,
}

impl ConnGuard {
    fn new(handle: client::Handle<SshClientHandler>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    fn as_handle(&mut self) -> &mut client::Handle<SshClientHandler> {
        self.handle.as_mut().expect("ConnGuard consumed twice")
    }

    fn into_inner(mut self) -> client::Handle<SshClientHandler> {
        self.handle.take().expect("ConnGuard consumed twice")
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take()
            && let Ok(rt) = tokio::runtime::Handle::try_current()
        {
            rt.spawn(async move {
                let _ = handle
                    .disconnect(russh::Disconnect::ByApplication, "", "en")
                    .await;
            });
        }
    }
}

pub(crate) struct SshClientHandler {
    checker: std::sync::Arc<HostKeyChecker>,
}

impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(ssh_key::HashAlg::Sha256).to_string();
        let raw = key.to_openssh().unwrap_or_else(|_| String::new());
        let key_line = raw.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        if key_line.is_empty() {
            return Ok(false);
        }
        // Re-read file in case it was changed externally between the
        // SshManager reload and this handshake.
        self.checker.known.reload();
        let stored_line = self
            .checker
            .known
            .find(&self.checker.host, self.checker.port);
        match stored_line {
            Some(stored) if stored == key_line => Ok(true),
            Some(_) => {
                let stored_fp = self
                    .checker
                    .known
                    .fingerprint_for(&self.checker.host, self.checker.port);
                let mut guard = self.checker.presented.lock().unwrap();
                *guard = Some(crate::ssh_known_hosts::PresentedKey {
                    fingerprint,
                    key_line,
                    stored_fp,
                });
                Ok(false)
            }
            None => {
                let mut guard = self.checker.presented.lock().unwrap();
                *guard = Some(crate::ssh_known_hosts::PresentedKey {
                    fingerprint,
                    key_line,
                    stored_fp: None,
                });
                Ok(false)
            }
        }
    }
}

/// Outcome of an ssh-agent public-key authentication attempt.
enum AgentAuthOutcome {
    /// An agent identity was accepted; the caller owns an authenticated session.
    Authenticated,
    /// No agent could be reached or it held no identities.
    Unavailable,
    /// The agent held identities, but the server rejected every one.
    AllRejected,
}

/// Public-key authentication failure categories, so callers can distinguish an
/// agent rejection from a key-file rejection for better diagnostics.
#[derive(Debug)]
pub enum PubkeyAuthError {
    /// The ssh-agent was available and held identities, but the server
    /// rejected every one of them.
    AgentRejected,
    /// No agent identity or key file was accepted; the last key-file error.
    KeyRejected(String),
    /// Neither an agent nor default key files were available.
    NoAuthMethods,
}

impl std::fmt::Display for PubkeyAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AgentRejected => write!(f, "Agent authentication failed"),
            Self::KeyRejected(e) => f.write_str(e),
            Self::NoAuthMethods => {
                write!(
                    f,
                    "No ssh-agent or SSH key files available for authentication"
                )
            }
        }
    }
}

impl std::error::Error for PubkeyAuthError {}

pub struct SftpFs {
    session: Arc<client::Handle<SshClientHandler>>,
    sftp: Arc<SftpSession>,
    _host: String,
    _user: String,
    password: Option<SecretString>,
    prefix: String,
}

impl SftpFs {
    /// Connect with password authentication and return a ready `SftpFs`.
    ///
    /// Takes the owned `SecretString` so the same zeroized buffer can be
    /// exposed briefly at the russh boundary and then stored on `SftpFs`
    /// without re-materializing a second copy.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH connection fails or authentication is rejected.
    pub async fn connect_password(
        host: &str,
        port: u16,
        user: &str,
        password: SecretString,
        read_timeout_secs: u64,
        keepalive_interval: u32,
        checker: std::sync::Arc<crate::ssh_known_hosts::HostKeyChecker>,
    ) -> Result<Self> {
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(read_timeout_secs)),
            keepalive_interval: Some(Duration::from_secs(u64::from(keepalive_interval))),
            ..Default::default()
        });
        let checker_clone = std::sync::Arc::clone(&checker);
        let handle = match client::connect(
            config,
            (host, port),
            SshClientHandler {
                checker: checker_clone,
            },
        )
        .await
        {
            Ok(h) => h,
            Err(e) => {
                if matches!(e, russh::Error::UnknownKey)
                    && let Some(presented) = checker.take_presented()
                {
                    return Err(HostKeyMismatch {
                        presented_fp: presented.fingerprint,
                        stored_fp: presented.stored_fp,
                        key_line: presented.key_line,
                    }
                    .into());
                }
                return Err(SshConnectError {
                    detail: format!("SSH connect failed: {e}"),
                }
                .into());
            }
        };

        let mut guard = ConnGuard::new(handle);
        {
            let exposed = password.expose_secret();
            let ok = guard
                .as_handle()
                .authenticate_password(user, exposed)
                .await
                .map_err(|e| PasswordAuthError {
                    detail: format!("Password authentication error: {e}"),
                })?;
            if !matches!(ok, russh::client::AuthResult::Success) {
                return Err(PasswordAuthError {
                    detail: "Password authentication rejected by server".to_string(),
                }
                .into());
            }
        }

        Self::from_handle(
            guard.into_inner(),
            host.to_string(),
            user.to_string(),
            Some(password),
        )
        .await
    }

    /// Connect using public-key authentication: tries the ssh-agent and then
    /// the default key files in `~/.ssh/`. The agent is discovered on Unix via
    /// `SSH_AUTH_SOCK`, on Windows via the OpenSSH named pipe with a Pageant
    /// fallback.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH connection fails or all identities are
    /// rejected.
    pub async fn connect_pubkey(
        host: &str,
        port: u16,
        user: &str,
        read_timeout_secs: u64,
        keepalive_interval: u32,
        checker: std::sync::Arc<crate::ssh_known_hosts::HostKeyChecker>,
    ) -> Result<Self> {
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(read_timeout_secs)),
            keepalive_interval: Some(Duration::from_secs(u64::from(keepalive_interval))),
            ..Default::default()
        });
        let checker_clone = std::sync::Arc::clone(&checker);
        let handle = match client::connect(
            config,
            (host, port),
            SshClientHandler {
                checker: checker_clone,
            },
        )
        .await
        {
            Ok(h) => h,
            Err(e) => {
                if matches!(e, russh::Error::UnknownKey)
                    && let Some(presented) = checker.take_presented()
                {
                    return Err(HostKeyMismatch {
                        presented_fp: presented.fingerprint,
                        stored_fp: presented.stored_fp,
                        key_line: presented.key_line,
                    }
                    .into());
                }
                return Err(SshConnectError {
                    detail: format!("SSH connect failed: {e}"),
                }
                .into());
            }
        };

        let mut guard = ConnGuard::new(handle);
        let mut agent_rejected = false;
        match Self::agent_auth(guard.as_handle(), user).await {
            AgentAuthOutcome::Authenticated => {
                return Self::from_handle(
                    guard.into_inner(),
                    host.to_string(),
                    user.to_string(),
                    None,
                )
                .await;
            }
            AgentAuthOutcome::AllRejected => agent_rejected = true,
            AgentAuthOutcome::Unavailable => {}
        }

        let keys = find_default_ssh_keys();
        if keys.is_empty() {
            return Err(PubkeyAuthError::NoAuthMethods.into());
        }

        let mut last_err = anyhow!("All key authentication attempts failed");
        for key_path in &keys {
            // Load the key via russh::keys::load_secret_key.
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

            match guard.as_handle().authenticate_publickey(user, pk).await {
                Ok(russh::client::AuthResult::Success) => {
                    return Self::from_handle(
                        guard.into_inner(),
                        host.to_string(),
                        user.to_string(),
                        None,
                    )
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
        Err(if agent_rejected {
            PubkeyAuthError::AgentRejected.into()
        } else {
            PubkeyAuthError::KeyRejected(last_err.to_string()).into()
        })
    }

    /// Attempt public-key authentication via the ssh-agent. Returns
    /// `Authenticated` when an identity was accepted (the caller then owns a
    /// successfully authenticated session), `AllRejected` when the agent held
    /// identities but the server rejected every one, and `Unavailable` when no
    /// agent could be reached or it had no identities.
    async fn agent_auth(
        handle: &mut client::Handle<SshClientHandler>,
        user: &str,
    ) -> AgentAuthOutcome {
        let Ok(mut agent) = Self::connect_agent().await else {
            return AgentAuthOutcome::Unavailable;
        };
        let Ok(identities) = agent.request_identities().await else {
            return AgentAuthOutcome::Unavailable;
        };
        if identities.is_empty() {
            return AgentAuthOutcome::Unavailable;
        }

        let rsa_hash: Option<russh::keys::HashAlg> = handle
            .best_supported_rsa_hash()
            .await
            .unwrap_or(None)
            .flatten();

        let mut rejected_any = false;
        for identity in &identities {
            let result = match identity {
                AgentIdentity::Certificate { certificate, .. } => {
                    handle
                        .authenticate_certificate_with(
                            user,
                            certificate.clone(),
                            rsa_hash,
                            &mut agent,
                        )
                        .await
                }
                AgentIdentity::PublicKey { key, .. } => {
                    handle
                        .authenticate_publickey_with(user, key.clone(), rsa_hash, &mut agent)
                        .await
                }
            };
            if matches!(result, Ok(russh::client::AuthResult::Success)) {
                return AgentAuthOutcome::Authenticated;
            }
            rejected_any = true;
        }
        if rejected_any {
            AgentAuthOutcome::AllRejected
        } else {
            AgentAuthOutcome::Unavailable
        }
    }

    /// Connect to the platform's ssh-agent, boxing the differing stream types
    /// so all connect paths share a single `AgentClient` type.
    async fn connect_agent() -> anyhow::Result<AgentClient<Box<dyn AgentStream + Send + Unpin>>> {
        #[cfg(unix)]
        {
            Ok(AgentClient::connect_env()
                .await
                .map_err(|e| anyhow!("Failed to connect to SSH agent: {e}"))?
                .dynamic())
        }
        #[cfg(windows)]
        {
            match AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
                Ok(agent) => Ok(agent.dynamic()),
                Err(pipe_err) => AgentClient::connect_pageant()
                    .await
                    .map_err(|pageant_err| {
                        anyhow!(
                            "Failed to connect to SSH agent: {pipe_err}; Pageant: {pageant_err}"
                        )
                    })
                    .map(AgentClient::dynamic),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(anyhow!("SSH agent is not supported on this platform"))
        }
    }

    /// Open the SFTP subsystem over a freshly-authenticated session handle.
    async fn from_handle(
        handle: client::Handle<SshClientHandler>,
        host: String,
        user: String,
        password: Option<SecretString>,
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

    /// Run an async SFTP closure from a synchronous context using
    /// `block_in_place` + the current Tokio handle.
    fn run_async<F, Fut, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(Arc<SftpSession>) -> Fut,
        Fut: std::future::Future<Output = Result<R>>,
    {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| anyhow!("No Tokio runtime available for SFTP operation"))?;
        if handle.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
            return Err(anyhow!(
                "SFTP operations require the multi-threaded Tokio runtime"
            ));
        }
        tokio::task::block_in_place(|| handle.block_on(f(Arc::clone(&self.sftp))))
    }

    /// Recursively delete a remote directory via SFTP.
    async fn delete_recursive(sftp: &SftpSession, path: &str) -> Result<()> {
        let entries = sftp
            .read_dir(path)
            .await
            .map_err(|e| anyhow!("readdir {path}: {e}"))?;
        for entry in entries {
            let name = entry.file_name().clone();
            if is_dot_or_dotdot(&name) {
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
            let path_str = normalize_sftp_path(path);
            let entries = sftp
                .read_dir(&path_str)
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
                    attributes: format_sftp_permissions(meta.permissions.unwrap_or(0)),
                    selected: false,
                });
            }
            Ok(result)
        })
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        self.run_async(|sftp| async move {
            let p = normalize_sftp_path(path);
            sftp.create_dir(p)
                .await
                .map_err(|e| anyhow!("Failed to create directory: {e}"))
        })
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        self.run_async(|sftp| async move {
            let p = normalize_sftp_path(path);
            sftp.create(p)
                .await
                .map_err(|e| anyhow!("Failed to create file: {e}"))?;
            Ok(())
        })
    }

    fn delete(&self, path: &Path, recursive: bool) -> Result<()> {
        self.run_async(|sftp| async move {
            let p = normalize_sftp_path(path);
            let meta = sftp
                .metadata(&p)
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
            sftp.rename(normalize_sftp_path(from), normalize_sftp_path(to))
                .await
                .map_err(|e| anyhow!("Failed to rename: {e}"))
        })
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.run_async(|sftp| async move {
            sftp.read(normalize_sftp_path(path))
                .await
                .map_err(|e| anyhow!("Failed to read file: {e}"))
        })
    }

    fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.run_async(|sftp| async move {
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
        })
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        self.run_async(|sftp| async move {
            sftp.write(normalize_sftp_path(path), &data)
                .await
                .map_err(|e| anyhow!("Failed to write file: {e}"))
        })
    }

    fn write_file_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        self.run_async(|sftp| async move {
            let p = normalize_sftp_path(path);
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
            sftp.try_exists(normalize_sftp_path(path))
                .await
                .map_err(|e| anyhow!("{e}"))
        })
        .unwrap_or(false)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.run_async(|sftp| async move {
            let meta = sftp
                .metadata(normalize_sftp_path(path))
                .await
                .map_err(|e| anyhow!("{e}"))?;
            Ok(meta.is_dir())
        })
        .unwrap_or(false)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        self.run_async(|sftp| async move {
            let s = sftp
                .canonicalize(normalize_sftp_path(path))
                .await
                .map_err(|e| anyhow!("Failed to canonicalize: {e}"))?;
            Ok(PathBuf::from(s))
        })
    }

    fn get_permissions(&self, path: &Path) -> Option<u32> {
        self.run_async(|sftp| async move {
            let meta = sftp
                .metadata(normalize_sftp_path(path))
                .await
                .map_err(|e| anyhow!("{e}"))?;
            Ok(meta.permissions.map(|p| p & 0o777))
        })
        .ok()
        .flatten()
    }

    fn set_permissions(&self, path: &Path, mode: u32) -> bool {
        self.run_async(|sftp| async move {
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
            .map_err(|e| anyhow!("setstat failed: {e}"))
        })
        .is_ok()
    }

    fn get_modified_time(&self, path: &Path) -> Option<std::time::SystemTime> {
        self.run_async(|sftp| async move {
            let meta = sftp
                .metadata(normalize_sftp_path(path))
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
            .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX));
        self.run_async(|sftp| async move {
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
            .map_err(|e| anyhow!("setstat mtime failed: {e}"))
        })
        .is_ok()
    }

    fn context_key(&self) -> String {
        self.prefix.clone()
    }

    fn get_password(&self) -> Option<SecretString> {
        self.password.clone()
    }

    fn display_path(&self, path: &Path) -> String {
        normalize_sftp_path(path)
    }

    async fn calc_dir_size(&self, path: &Path) -> anyhow::Result<u64> {
        let path_str = normalize_sftp_path(path);
        let cmd = build_du_command(&path_str);

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
        let src_str = normalize_sftp_path(src);
        let dest_path = dest.to_path_buf();

        let (total_size, perms, is_dir) = match self.sftp.metadata(src_str.as_str()).await {
            Ok(m) => (
                m.size.unwrap_or(0),
                m.permissions.map(|p| p & 0o777),
                (m.permissions.unwrap_or(0) & 0o170_000) == 0o040_000,
            ),
            Err(e) => return Some(Err(anyhow!("Failed to stat: {e}"))),
        };

        if is_dir {
            return None;
        }

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

        let chunk_size = calculate_optimal_chunk_size(total_size);
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
        let dest_str = normalize_sftp_path(dest);
        let src_path = src.to_path_buf();

        if src_fs.is_dir(&src_path).await.unwrap_or(false) {
            return None;
        }

        let total_size = match src_fs.get_size(&src_path).await {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        };
        let src_perms = src_fs.get_permissions(&src_path).await;
        let chunk_size = calculate_optimal_chunk_size(total_size);

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
