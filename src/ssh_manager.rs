use crate::config::SshConfig;
use secrecy::SecretString;
use std::time::Duration;

/// Authentication method a session was established with; reconnection must use
/// the same method unless the user explicitly falls back to a password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    Password,
    Pubkey,
}

#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_id: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub target_path: Option<String>,
    pub auth_method: AuthMethod,
    /// Set while a reconnection attempt is in flight so concurrent requests
    /// for the same session are rejected instead of duplicating attempts.
    pub reconnecting: bool,
}

#[derive(Debug)]
pub enum NetworkError {
    ConnectionRefused,
    ConnectionTimedOut,
    HostUnreachable,
    NoRoute,
    InvalidAddress,
    Other(String),
}

#[derive(Debug)]
pub enum AuthError {
    KeyAuthFailed,
    PasswordAuthFailed,
    NoAuthMethodsAvailable,
    AgentError(String),
}

#[derive(Debug)]
pub enum SshError {
    Network(NetworkError),
    Auth(AuthError),
    HostKey {
        host: String,
        port: u16,
        presented: String,
        stored: Option<String>,
        key_line: String,
    },
    Connection(String),
    InvalidInput(String),
    Internal(String),
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshError::Network(e) => write!(f, "Network error: {e}"),
            SshError::Auth(e) => write!(f, "Authentication error: {e}"),
            SshError::HostKey {
                host,
                port,
                presented,
                stored,
                ..
            } => {
                if let Some(stored) = stored {
                    write!(
                        f,
                        "Host key for {host}:{port} changed (was {stored}, now {presented})"
                    )
                } else {
                    write!(f, "Unknown host key for {host}:{port}: {presented}")
                }
            }
            SshError::Connection(s) => write!(f, "Connection error: {s}"),
            SshError::InvalidInput(s) => write!(f, "Invalid input: {s}"),
            SshError::Internal(s) => write!(f, "Internal error: {s}"),
        }
    }
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::ConnectionRefused => write!(f, "Connection refused"),
            NetworkError::ConnectionTimedOut => write!(f, "Connection timed out"),
            NetworkError::HostUnreachable => write!(f, "Host unreachable"),
            NetworkError::NoRoute => write!(f, "No route to host"),
            NetworkError::InvalidAddress => write!(f, "Invalid address"),
            NetworkError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::KeyAuthFailed => write!(f, "Key authentication failed"),
            AuthError::PasswordAuthFailed => write!(f, "Password authentication failed"),
            AuthError::NoAuthMethodsAvailable => write!(f, "No authentication methods available"),
            AuthError::AgentError(s) => write!(f, "Agent error: {s}"),
        }
    }
}

#[derive(Clone)]
pub struct SshManager {
    base_backoff: Duration,
    backoff_factor: f64,
    max_backoff: Duration,
    jitter_pct: f64,
    pub keepalive_interval: u32,
    pub read_timeout_secs: u64,
    pub connect_timeout_secs: u64,
    sessions: std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, SessionState>>>,
    password_cache:
        std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, SecretString>>>,
    known_hosts: std::sync::Arc<crate::ssh_known_hosts::KnownHosts>,
}

impl SshManager {
    #[must_use]
    pub fn new(ssh_config: Option<&SshConfig>) -> Self {
        let keepalive_interval = ssh_config.and_then(|c| c.keepalive_interval).unwrap_or(10);
        let read_timeout_secs = ssh_config.and_then(|c| c.read_timeout_secs).unwrap_or(15);
        let connect_timeout_secs = ssh_config.and_then(|c| c.connect_timeout_secs).unwrap_or(30);

        Self {
            base_backoff: Duration::from_secs(1),
            backoff_factor: 2.0,
            max_backoff: Duration::from_mins(1),
            jitter_pct: 0.2,
            keepalive_interval,
            read_timeout_secs,
            connect_timeout_secs,
            sessions: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            password_cache: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            known_hosts: std::sync::Arc::new(crate::ssh_known_hosts::KnownHosts::new()),
        }
    }

    #[must_use]
    pub fn known_hosts(&self) -> std::sync::Arc<crate::ssh_known_hosts::KnownHosts> {
        std::sync::Arc::clone(&self.known_hosts)
    }

    /// Attempts to connect to a remote host using SSH key authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or authentication with any found key fails.
    pub async fn try_connect_with_keys(
        &self,
        host: String,
        port: u16,
        user: String,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let session_id = Self::generate_session_id(&host, port);
        let timeout = Duration::from_secs(self.connect_timeout_secs);

        let fs = tokio::time::timeout(
            timeout,
            self.try_connect_with_keys_backend(host, port, user),
        )
        .await
        .map_err(|_| SshError::Network(NetworkError::ConnectionTimedOut))??;

        Ok((session_id, fs))
    }

    async fn try_connect_with_keys_backend(
        &self,
        host: String,
        port: u16,
        user: String,
    ) -> Result<crate::fs::fs_sftp::SftpFs, SshError> {
        self.known_hosts.reload();
        let checker = std::sync::Arc::new(crate::ssh_known_hosts::HostKeyChecker::new(
            host.clone(),
            port,
            std::sync::Arc::clone(&self.known_hosts),
        ));
        crate::fs::fs_sftp_russh::SftpFs::connect_pubkey(
            &host,
            port,
            &user,
            self.read_timeout_secs,
            self.keepalive_interval,
            checker,
        )
        .await
        .map_err(|e| {
            if let Some(m) = e.downcast_ref::<crate::fs::fs_sftp_russh::HostKeyMismatch>() {
                return SshError::HostKey {
                    host: host.clone(),
                    port,
                    presented: m.presented_fp.clone(),
                    stored: m.stored_fp.clone(),
                    key_line: m.key_line.clone(),
                };
            }
            if let Some(c) = e.downcast_ref::<crate::fs::fs_sftp_russh::SshConnectError>() {
                return SshError::Network(Self::classify_connect_error(&c.detail));
            }
            match e.downcast_ref::<crate::fs::fs_sftp_russh::PubkeyAuthError>() {
                Some(crate::fs::fs_sftp_russh::PubkeyAuthError::AgentRejected) => SshError::Auth(
                    AuthError::AgentError("Agent authentication failed".to_string()),
                ),
                Some(crate::fs::fs_sftp_russh::PubkeyAuthError::NoAuthMethods) => {
                    SshError::Auth(AuthError::NoAuthMethodsAvailable)
                }
                Some(crate::fs::fs_sftp_russh::PubkeyAuthError::KeyRejected(_)) => {
                    SshError::Auth(AuthError::KeyAuthFailed)
                }
                None => SshError::Connection(e.to_string()),
            }
        })
    }

    /// Maps a transport connection failure to the most specific
    /// `NetworkError` variant based on the error text.
    fn classify_connect_error(detail: &str) -> NetworkError {
        let lower = detail.to_lowercase();
        if lower.contains("timed out") {
            NetworkError::ConnectionTimedOut
        } else if lower.contains("refused") {
            NetworkError::ConnectionRefused
        } else if lower.contains("no route") {
            NetworkError::NoRoute
        } else if lower.contains("unreachable") {
            NetworkError::HostUnreachable
        } else {
            NetworkError::Other(detail.to_string())
        }
    }

    #[must_use]
    pub fn compute_backoff(&self, attempt: u32) -> Duration {
        let attempt = attempt.max(1);
        let mut secs =
            self.base_backoff.as_secs_f64() * self.backoff_factor.powf(f64::from(attempt - 1));
        if secs > self.max_backoff.as_secs_f64() {
            secs = self.max_backoff.as_secs_f64();
        }

        let u = rand::random::<f64>();
        let jitter = 1.0 + (u * 2.0 - 1.0) * self.jitter_pct;
        secs = (secs * jitter).min(self.max_backoff.as_secs_f64());
        Duration::from_secs_f64(secs)
    }

    #[must_use]
    pub fn generate_session_id(host: &str, port: u16) -> String {
        use std::time::SystemTime;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        format!("ssh_{host}_{port}_{now}")
    }

    /// Connects to a remote host via SSH.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect_ssh(
        &self,
        host: String,
        port: u16,
        user: String,
        password: SecretString,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let session_id = Self::generate_session_id(&host, port);
        let timeout = Duration::from_secs(self.connect_timeout_secs);

        let fs = tokio::time::timeout(
            timeout,
            self.connect_ssh_backend(host, port, user, password),
        )
        .await
        .map_err(|_| SshError::Network(NetworkError::ConnectionTimedOut))??;

        Ok((session_id, fs))
    }

    async fn connect_ssh_backend(
        &self,
        host: String,
        port: u16,
        user: String,
        password: SecretString,
    ) -> Result<crate::fs::fs_sftp::SftpFs, SshError> {
        self.known_hosts.reload();
        let checker = std::sync::Arc::new(crate::ssh_known_hosts::HostKeyChecker::new(
            host.clone(),
            port,
            std::sync::Arc::clone(&self.known_hosts),
        ));
        crate::fs::fs_sftp_russh::SftpFs::connect_password(
            &host,
            port,
            &user,
            password,
            self.read_timeout_secs,
            self.keepalive_interval,
            checker,
        )
        .await
        .map_err(|e| {
            if let Some(m) = e.downcast_ref::<crate::fs::fs_sftp_russh::HostKeyMismatch>() {
                return SshError::HostKey {
                    host: host.clone(),
                    port,
                    presented: m.presented_fp.clone(),
                    stored: m.stored_fp.clone(),
                    key_line: m.key_line.clone(),
                };
            }
            if let Some(c) = e.downcast_ref::<crate::fs::fs_sftp_russh::SshConnectError>() {
                return SshError::Network(Self::classify_connect_error(&c.detail));
            }
            if e.downcast_ref::<crate::fs::fs_sftp_russh::PasswordAuthError>().is_some() {
                return SshError::Auth(AuthError::PasswordAuthFailed);
            }
            SshError::Connection(e.to_string())
        })
    }

    /// Connects with password authentication, registers the resulting session
    /// and caches the password so background reconnection can retry without
    /// user interaction.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection or authentication fails.
    pub async fn connect_password_session(
        &self,
        host: String,
        port: u16,
        user: String,
        password: SecretString,
        target_path: Option<String>,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let (session_id, fs) =
            self.connect_ssh(host.clone(), port, user.clone(), password.clone())
                .await?;
        self.register_session(
            session_id.clone(),
            host,
            port,
            user,
            target_path,
            AuthMethod::Password,
        );
        self.cache_password(&session_id, password);
        Ok((session_id, fs))
    }

    /// Connects with public-key authentication and registers the resulting
    /// session.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection or key authentication fails.
    pub async fn connect_pubkey_session(
        &self,
        host: String,
        port: u16,
        user: String,
        target_path: Option<String>,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let (session_id, fs) = self
            .try_connect_with_keys(host.clone(), port, user.clone())
            .await?;
        self.register_session(
            session_id.clone(),
            host,
            port,
            user,
            target_path,
            AuthMethod::Pubkey,
        );
        Ok((session_id, fs))
    }

    /// Registers a new SSH session.
    pub fn register_session(
        &self,
        session_id: String,
        host: String,
        port: u16,
        user: String,
        target_path: Option<String>,
        auth_method: AuthMethod,
    ) {
        let state = SessionState {
            session_id: session_id.clone(),
            host,
            port,
            user,
            target_path,
            auth_method,
            reconnecting: false,
        };
        let mut sessions = self.sessions.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.insert(session_id, state);
    }

    /// Unregisters an SSH session.
    pub fn unregister_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.remove(session_id);
    }

    /// Gets a session by ID.
    #[must_use]
    pub fn get_session(&self, session_id: &str) -> Option<SessionState> {
        let sessions = self.sessions.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.get(session_id).cloned()
    }

    /// Gets all sessions.
    #[must_use]
    pub fn get_all_sessions(&self) -> Vec<SessionState> {
        let sessions = self.sessions.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.values().cloned().collect()
    }

    /// Marks a session as (not) currently being reconnected, so concurrent
    /// reconnect requests for the same session are rejected instead of
    /// duplicating connection attempts.
    pub fn set_reconnecting(&self, session_id: &str, reconnecting: bool) {
        let mut sessions = self.sessions.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(s) = sessions.get_mut(session_id) {
            s.reconnecting = reconnecting;
        }
    }

    /// Caches a password for a session.
    pub fn cache_password(&self, session_id: &str, password: SecretString) {
        let mut cache = self.password_cache.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.insert(session_id.to_string(), password);
    }

    /// Gets a cached password for a session.
    #[must_use]
    pub fn get_cached_password(&self, session_id: &str) -> Option<SecretString> {
        let cache = self.password_cache.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.get(session_id).cloned()
    }

    /// Removes and returns the cached password for a session, so the caller
    /// owns the same zeroized buffer without leaving a stale copy behind.
    #[must_use]
    pub fn take_cached_password(&self, session_id: &str) -> Option<SecretString> {
        let mut cache = self.password_cache.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.remove(session_id)
    }

    /// Clears a cached password for a session.
    pub fn clear_password(&self, session_id: &str) {
        let mut cache = self.password_cache.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.remove(session_id);
    }

    /// Clears all cached passwords.
    pub fn clear_all_passwords(&self) {
        let mut cache = self.password_cache.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.clear();
    }

    /// Reconnects a session with a password, replacing the old session entry
    /// on success.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is unknown, a reconnection is already
    /// in flight for it, or if reconnection fails.
    pub async fn reconnect_session(
        &self,
        session_id: &str,
        password: SecretString,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let session = self.get_session(session_id).ok_or_else(|| {
            SshError::InvalidInput(format!("Session {session_id} not found for reconnection"))
        })?;
        if session.reconnecting {
            return Err(SshError::Internal(
                "Reconnection already in progress for this session".to_string(),
            ));
        }
        self.set_reconnecting(session_id, true);
        let _guard = ReconnectGuard {
            manager: self,
            session_id: session_id.to_string(),
            active: true,
        };

        let (new_session_id, fs) = self
            .reconnect_with_backoff(
                session.host.clone(),
                session.port,
                session.user.clone(),
                &password,
                Some(3),
            )
            .await?;

        self.clear_password(session_id);
        self.unregister_session(session_id);
        self.register_session(
            new_session_id.clone(),
            session.host,
            session.port,
            session.user,
            session.target_path,
            AuthMethod::Password,
        );
        self.cache_password(&new_session_id, password);

        Ok((new_session_id, fs))
    }

    /// Reconnects a public-key authenticated session using key/agent
    /// authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is unknown, a reconnection is already
    /// in flight for it, or if reconnection fails.
    pub async fn reconnect_session_with_keys(
        &self,
        session_id: &str,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let session = self.get_session(session_id).ok_or_else(|| {
            SshError::InvalidInput(format!("Session {session_id} not found for reconnection"))
        })?;
        if session.reconnecting {
            return Err(SshError::Internal(
                "Reconnection already in progress for this session".to_string(),
            ));
        }
        self.set_reconnecting(session_id, true);
        let _guard = ReconnectGuard {
            manager: self,
            session_id: session_id.to_string(),
            active: true,
        };

        let (new_session_id, fs) = self
            .reconnect_keys_with_backoff(
                session.host.clone(),
                session.port,
                session.user.clone(),
                Some(3),
            )
            .await?;

        self.unregister_session(session_id);
        self.register_session(
            new_session_id.clone(),
            session.host,
            session.port,
            session.user,
            session.target_path,
            AuthMethod::Pubkey,
        );

        Ok((new_session_id, fs))
    }

    /// Reconnects with a password using exponential backoff.
    ///
    /// # Errors
    ///
    /// Returns an error if reconnection fails after all attempts.
    pub async fn reconnect_with_backoff(
        &self,
        host: String,
        port: u16,
        user: String,
        password: &SecretString,
        max_attempts: Option<u32>,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let mut attempt = 1u32;
        let max_attempts = max_attempts.unwrap_or(u32::MAX);

        loop {
            match self
                .connect_ssh(host.clone(), port, user.clone(), password.clone())
                .await
            {
                Ok(result) => return Ok(result),
                Err(SshError::HostKey { .. }) => {
                    return Err(SshError::Internal(format!(
                        "Reconnection failed after {attempt} attempts: host key mismatch"
                    )));
                }
                Err(e) if attempt >= max_attempts => {
                    return Err(SshError::Internal(format!(
                        "Reconnection failed after {attempt} attempts: {e}"
                    )));
                }
                Err(_) => {
                    let delay = self.compute_backoff(attempt);
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    /// Reconnects with key authentication using exponential backoff.
    ///
    /// # Errors
    ///
    /// Returns an error if reconnection fails after all attempts.
    pub async fn reconnect_keys_with_backoff(
        &self,
        host: String,
        port: u16,
        user: String,
        max_attempts: Option<u32>,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let mut attempt = 1u32;
        let max_attempts = max_attempts.unwrap_or(u32::MAX);

        loop {
            match self.try_connect_with_keys(host.clone(), port, user.clone()).await {
                Ok(result) => return Ok(result),
                Err(SshError::HostKey { .. }) => {
                    return Err(SshError::Internal(format!(
                        "Reconnection failed after {attempt} attempts: host key mismatch"
                    )));
                }
                Err(e) if attempt >= max_attempts => {
                    return Err(SshError::Internal(format!(
                        "Reconnection failed after {attempt} attempts: {e}"
                    )));
                }
                Err(_) => {
                    let delay = self.compute_backoff(attempt);
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }
}

/// Clears the `reconnecting` flag on a session when the reconnect future ends,
/// including on error and cancellation.
struct ReconnectGuard<'a> {
    manager: &'a SshManager,
    session_id: String,
    active: bool,
}

impl Drop for ReconnectGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            self.manager.set_reconnecting(&self.session_id, false);
        }
    }
}

impl Default for SshManager {
    fn default() -> Self {
        SshManager::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn test_backoff_values_and_jitter_bounds() {
        let mgr = SshManager {
            base_backoff: Duration::from_secs(1),
            backoff_factor: 2.0,
            max_backoff: std::time::Duration::from_mins(1),
            jitter_pct: 0.0,
            keepalive_interval: 10,
            read_timeout_secs: 15,
            connect_timeout_secs: 30,
            sessions: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            password_cache: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            known_hosts: std::sync::Arc::new(crate::ssh_known_hosts::KnownHosts::with_path(
                std::path::PathBuf::from("/tmp/fm_test_known_hosts_backoff"),
            )),
        };

        assert_eq!(mgr.compute_backoff(1).as_secs(), 1);
        assert_eq!(mgr.compute_backoff(2).as_secs(), 2);
        assert_eq!(mgr.compute_backoff(3).as_secs(), 4);
        assert_eq!(mgr.compute_backoff(8).as_secs(), 60);

        let mgr_j = SshManager {
            jitter_pct: 0.2,
            ..mgr
        };

        let d = mgr_j.compute_backoff(3);
        let expected = 4.0;
        let low = expected * (1.0 - 0.2);
        let high = expected * (1.0 + 0.2);
        let got = d.as_secs_f64();
        assert!(
            got >= low && got <= high,
            "got {got} not in [{low}..{high}]"
        );

        // Jittered delay at the backoff cap must never exceed the cap.
        for _ in 0..100 {
            assert!(mgr_j.compute_backoff(8).as_secs_f64() <= 60.0);
            assert!(mgr_j.compute_backoff(8).as_secs_f64() >= 60.0 * (1.0 - 0.2));
        }
    }

    #[test]
    fn test_generate_session_id() {
        let id1 = SshManager::generate_session_id("example.com", 22);
        assert!(id1.starts_with("ssh_example.com_22_"));
        // Verify it contains a timestamp-like suffix
        let suffix = &id1["ssh_example.com_22_".len()..];
        assert!(
            suffix.parse::<u64>().is_ok(),
            "suffix should be numeric: {suffix}"
        );
    }

    #[test]
    fn test_session_registration_and_tracking() {
        let mgr = SshManager::new(None);

        // Register a session
        mgr.register_session(
            "session_test".to_string(),
            "example.com".to_string(),
            22,
            "user".to_string(),
            Some("/remote/path".to_string()),
            AuthMethod::Password,
        );

        // Verify session is registered
        let session = mgr.get_session("session_test");
        assert!(session.is_some());
        let session = session.unwrap();
        assert_eq!(session.host, "example.com");
        assert_eq!(session.port, 22);
        assert_eq!(session.user, "user");
        assert_eq!(session.auth_method, AuthMethod::Password);
        assert!(!session.reconnecting);

        // Reconnecting flag toggles and reads back
        mgr.set_reconnecting("session_test", true);
        assert!(mgr.get_session("session_test").unwrap().reconnecting);
        mgr.set_reconnecting("session_test", false);
        assert!(!mgr.get_session("session_test").unwrap().reconnecting);
        // Flag for an unknown session is a no-op
        mgr.set_reconnecting("unknown", true);

        // Unregister session
        mgr.unregister_session("session_test");
        assert!(mgr.get_session("session_test").is_none());
    }

    #[test]
    fn test_get_session_distinguishes_same_user_host() {
        let mgr = SshManager::new(None);

        mgr.register_session(
            "ssh_host1.com_22_1".to_string(),
            "host1.com".to_string(),
            22,
            "user".to_string(),
            Some("/path/a".to_string()),
            AuthMethod::Password,
        );
        mgr.register_session(
            "ssh_host1.com_22_2".to_string(),
            "host1.com".to_string(),
            22,
            "user".to_string(),
            Some("/path/b".to_string()),
            AuthMethod::Pubkey,
        );

        assert_eq!(
            mgr.get_session("ssh_host1.com_22_1")
                .unwrap()
                .target_path
                .as_deref(),
            Some("/path/a")
        );
        assert_eq!(
            mgr.get_session("ssh_host1.com_22_2")
                .unwrap()
                .target_path
                .as_deref(),
            Some("/path/b")
        );
    }

    #[test]
    fn test_get_all_sessions() {
        let mgr = SshManager::new(None);

        mgr.register_session(
            "s1".to_string(),
            "host1.com".to_string(),
            22,
            "u1".to_string(),
            None,
            AuthMethod::Password,
        );
        mgr.register_session(
            "s2".to_string(),
            "host2.com".to_string(),
            22,
            "u2".to_string(),
            None,
            AuthMethod::Pubkey,
        );

        let sessions = mgr.get_all_sessions();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_password_cache() {
        let mgr = SshManager::new(None);

        let session_id = "test_session";
        let password = "secret123";

        // Initially no cached password
        assert!(mgr.get_cached_password(session_id).is_none());

        mgr.cache_password(session_id, SecretString::new(password.to_string().into()));
        assert_eq!(
            mgr.get_cached_password(session_id)
                .map(|p| p.expose_secret().to_string()),
            Some(password.to_string())
        );

        // Clear password
        mgr.clear_password(session_id);
        assert!(mgr.get_cached_password(session_id).is_none());

        // Test with a different session to verify cache works independently
        let session_id2 = "test_session2";
        let password2 = "secret456";

        // Initially no cached password
        assert!(mgr.get_cached_password(session_id2).is_none());

        // Cache password
        mgr.cache_password(session_id2, SecretString::new(password2.to_string().into()));
        assert_eq!(
            mgr.get_cached_password(session_id2)
                .map(|p| p.expose_secret().to_string()),
            Some(password2.to_string())
        );

        // Clear password for session_id2
        mgr.clear_password(session_id2);
        assert!(mgr.get_cached_password(session_id2).is_none());

        mgr.cache_password(session_id, SecretString::new(password.to_string().into()));
        assert_eq!(
            mgr.get_cached_password(session_id)
                .map(|p| p.expose_secret().to_string()),
            Some(password.to_string())
        );

        // Clear password
        mgr.clear_password(session_id);
        assert!(mgr.get_cached_password(session_id).is_none());

        // Test multiple sessions
        mgr.cache_password("session1", SecretString::new("pass1".to_string().into()));
        mgr.cache_password("session2", SecretString::new("pass2".to_string().into()));
        assert_eq!(
            mgr.get_cached_password("session1")
                .map(|p| p.expose_secret().to_string()),
            Some("pass1".to_string())
        );
        assert_eq!(
            mgr.get_cached_password("session2")
                .map(|p| p.expose_secret().to_string()),
            Some("pass2".to_string())
        );

        // Clear all passwords
        mgr.clear_all_passwords();
        assert!(mgr.get_cached_password("session1").is_none());
        assert!(mgr.get_cached_password("session2").is_none());
    }
}
