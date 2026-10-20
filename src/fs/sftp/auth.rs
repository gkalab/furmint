//! SSH transport, authentication and ssh-agent plumbing.

use super::SftpFs;
use super::errors::{HostKeyMismatch, PasswordAuthError, PubkeyAuthError, SshConnectError};
use crate::fs::utils::find_default_ssh_keys;
use crate::ssh::known_hosts::HostKeyChecker;
use anyhow::{Result, anyhow};
use russh::client;
use russh::keys::PublicKeyOrCertificate;
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::ssh_key;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
use std::time::Duration;

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

    #[allow(clippy::unused_async_trait_impl)]
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let public_key = key.public_key();
        let fingerprint = public_key.fingerprint(ssh_key::HashAlg::Sha256).to_string();
        // Surface a diagnostic if the server key cannot be encoded to the
        // OpenSSH form: returning Ok(false) here would abort the connection
        // with a bare "UnknownKey" error and no explanation.
        let raw = match public_key.to_openssh() {
            Ok(r) => r,
            Err(e) => {
                self.checker
                    .set_diag(format!("failed to encode server key for host check: {e}"));
                return Err(russh::Error::Inconsistent);
            }
        };
        let key_line = raw.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        if key_line.is_empty() {
            self.checker
                .set_diag("server presented an empty or unencodable public key".to_string());
            return Err(russh::Error::Inconsistent);
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
                *guard = Some(crate::ssh::known_hosts::PresentedKey {
                    fingerprint,
                    key_line,
                    stored_fp,
                });
                Ok(false)
            }
            None => {
                let mut guard = self.checker.presented.lock().unwrap();
                *guard = Some(crate::ssh::known_hosts::PresentedKey {
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

impl SftpFs {
    /// Establish the SSH transport (TCP connect plus KEX and handshake) and map
    /// failures onto the shared error types. Both authentication paths build on
    /// this; only the authentication step itself differs between them.
    ///
    /// # Errors
    ///
    /// Returns [`HostKeyMismatch`] if the server key is unknown or changed, or
    /// [`SshConnectError`] for any other transport failure.
    async fn connect_transport(
        host: &str,
        port: u16,
        read_timeout_secs: u64,
        keepalive_interval: u32,
        checker: Arc<HostKeyChecker>,
    ) -> Result<client::Handle<SshClientHandler>> {
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(read_timeout_secs)),
            keepalive_interval: Some(Duration::from_secs(u64::from(keepalive_interval))),
            ..Default::default()
        });
        let checker_clone = Arc::clone(&checker);
        match client::connect(
            config,
            (host, port),
            SshClientHandler {
                checker: checker_clone,
            },
        )
        .await
        {
            Ok(h) => Ok(h),
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
                let detail = match checker.take_diag() {
                    Some(d) => format!("SSH connect failed: {d}"),
                    None => format!("SSH connect failed: {e}"),
                };
                Err(SshConnectError { detail }.into())
            }
        }
    }

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
        checker: Arc<HostKeyChecker>,
    ) -> Result<Self> {
        let handle =
            Self::connect_transport(host, port, read_timeout_secs, keepalive_interval, checker)
                .await?;

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
            port,
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
        checker: Arc<HostKeyChecker>,
    ) -> Result<Self> {
        let handle =
            Self::connect_transport(host, port, read_timeout_secs, keepalive_interval, checker)
                .await?;

        let mut guard = ConnGuard::new(handle);
        let mut agent_rejected = false;
        match Self::agent_auth(guard.as_handle(), user).await {
            AgentAuthOutcome::Authenticated => {
                return Self::from_handle(
                    guard.into_inner(),
                    host.to_string(),
                    port,
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
                        port,
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
}
