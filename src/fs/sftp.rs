//! SFTP filesystem backend using russh + russh-sftp.
//!
//! This pure-Rust implementation is used on all platforms. It replaces the
//! `ssh2`/`libssh2` backend, which had KEX negotiation failures against modern
//! OpenSSH servers. Public-key authentication tries the ssh-agent before
//! falling back to key files in `~/.ssh/`: on Unix via `SSH_AUTH_SOCK`, on
//! Windows via the OpenSSH named pipe (`\\.\pipe\openssh-ssh-agent`) with a
//! `PuTTY` Pageant fallback.

mod auth;
mod errors;
mod provider;
mod transfer;

use anyhow::{Result, anyhow};
use russh::client;
use russh_sftp::client::SftpSession;
use secrecy::SecretString;
use std::sync::Arc;
use std::time::Duration;

pub(crate) use self::auth::SshClientHandler;
pub use self::errors::{HostKeyMismatch, PasswordAuthError, PubkeyAuthError, SshConnectError};

/// A single SSH session with an SFTP subsystem on top. The session owns one
/// transport for its whole lifetime: when that transport dies, every method
/// keeps failing until the session is replaced. Recovery is deliberately *not*
/// handled here — `SshManager::reconnect_session` rebuilds a fresh `SftpFs`
/// (re-authenticating, with password caching and backoff) and the app swaps it
/// into the tab, so reconnect never has to happen behind a provider call that
/// may need a password prompt or a host-key decision.
pub struct SftpFs {
    session: Arc<client::Handle<SshClientHandler>>,
    sftp: Arc<SftpSession>,
    host: String,
    user: String,
    port: u16,
    password: Option<SecretString>,
    display_prefix: String,
}

impl SftpFs {
    /// Upper bound on how long the remote `du` behind `calc_dir_size` may run.
    /// The session's `inactivity_timeout` does not cover this case: the server
    /// keeps answering keepalives while `du` walks a stalled mount, so without
    /// an explicit bound a size calculation could hang forever.
    const DU_TIMEOUT: Duration = Duration::from_secs(120);

    /// Open the SFTP subsystem over a freshly-authenticated session handle.
    async fn from_handle(
        handle: client::Handle<SshClientHandler>,
        host: String,
        port: u16,
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

        let display_prefix = format!("[{user}@{host}]");
        Ok(Self {
            session: Arc::new(handle),
            sftp: Arc::new(sftp),
            host,
            user,
            port,
            password,
            display_prefix,
        })
    }
}

impl Drop for SftpFs {
    fn drop(&mut self) {
        let sftp = Arc::clone(&self.sftp);
        let session = Arc::clone(&self.session);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = sftp.close().await;
                let _ = session
                    .disconnect(russh::Disconnect::ByApplication, "", "en")
                    .await;
            });
        }
    }
}

#[cfg(test)]
mod tests;
