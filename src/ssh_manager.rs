use crate::config::SshConfig;
#[cfg(unix)]
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_id: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub target_path: Option<String>,
}

#[derive(Debug)]
pub enum SshManagerError {
    JoinError(tokio::task::JoinError),
    HardKill,
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

impl From<std::io::Error> for NetworkError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind::{
            AddrInUse, AddrNotAvailable, BrokenPipe, ConnectionRefused, ConnectionReset,
            HostUnreachable, NetworkUnreachable, NotConnected, TimedOut,
        };

        match e.kind() {
            ConnectionRefused => NetworkError::ConnectionRefused,
            TimedOut => NetworkError::ConnectionTimedOut,
            NotConnected | AddrNotAvailable => NetworkError::InvalidAddress,
            NetworkUnreachable => NetworkError::NoRoute,
            HostUnreachable => NetworkError::HostUnreachable,
            AddrInUse | BrokenPipe | ConnectionReset | _ => NetworkError::Other(e.to_string()),
        }
    }
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
    InvalidInput(String),
    Internal(String),
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshError::Network(e) => write!(f, "Network error: {e}"),
            SshError::Auth(e) => write!(f, "Authentication error: {e}"),
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
    pub watchdog_secs: u64,
    sessions: std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, SessionState>>>,
    password_cache: std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, String>>>,
}

impl SshManager {
    #[must_use]
    pub fn new(ssh_config: Option<&SshConfig>) -> Self {
        let keepalive_interval = ssh_config.and_then(|c| c.keepalive_interval).unwrap_or(10);
        let read_timeout_secs = ssh_config.and_then(|c| c.read_timeout_secs).unwrap_or(15);
        let watchdog_secs = ssh_config.and_then(|c| c.watchdog_secs).unwrap_or(30);

        Self {
            base_backoff: Duration::from_secs(1),
            backoff_factor: 2.0,
            max_backoff: Duration::from_secs(60),
            jitter_pct: 0.2,
            keepalive_interval,
            read_timeout_secs,
            watchdog_secs,
            sessions: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            password_cache: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    /// Attempts to connect to a remote host using SSH key authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    /// # Errors
    ///
    /// Returns an error if the connection or public key authentication fails.
    #[cfg(unix)]
    pub async fn try_connect_with_keys(
        &self,
        host: String,
        port: u16,
        user: String,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let addr = format!("{host}:{port}");
        let tcp = tokio::net::TcpStream::connect(&addr)
            .await
            .map_err(|e| SshError::Network(e.into()))?;
        let tcp = tcp
            .into_std()
            .map_err(|e| SshError::Internal(e.to_string()))?;

        let grace = Duration::from_secs(self.read_timeout_secs);
        let hard = Duration::from_secs(self.watchdog_secs);
        let keepalive = self.keepalive_interval;
        let session_id = Self::generate_session_id(&host, port);

        self.spawn_blocking_with_watchdog(
            move || -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
                let mut sess =
                    ssh2::Session::new().map_err(|e| SshError::Internal(e.to_string()))?;
                sess.set_tcp_stream(tcp);
                sess.handshake()
                    .map_err(|e| {
                        let mut msg = e.to_string();
                        if matches!(e.code(), ssh2::ErrorCode::Session(-5)) {
                            msg.push_str(" (Check if your server requires modern SHA-2 RSA or Curve25519; ensure you're using ssh2 0.9.5+)");
                        }
                        SshError::Network(NetworkError::Other(msg))
                    })?;

                let agent_result = sess.agent();
                let mut agent_connected = false;
                let mut agent_identity_count = 0;

                if let Ok(mut agent) = agent_result {
                    agent_connected = agent.connect().is_ok();
                    if agent_connected
                        && agent.list_identities().is_ok()
                        && let Ok(identities) = agent.identities()
                    {
                        agent_identity_count = identities.len();
                        for identity in identities {
                            if agent.userauth(&user, &identity).is_ok() && sess.authenticated() {
                                sess.set_compress(true);
                                sess.set_keepalive(true, keepalive);
                                let fs = crate::fs::fs_sftp::SftpFs::new(
                                    sess,
                                    host.clone(),
                                    user.clone(),
                                    None,
                                );
                                return Ok((session_id.clone(), fs));
                            }
                        }
                    }
                }

                let keys = Self::find_default_ssh_keys();
                if keys.is_empty() && agent_identity_count == 0 {
                    return Err(SshError::Auth(AuthError::NoAuthMethodsAvailable));
                }

                for key in keys {
                    if sess.userauth_pubkey_file(&user, None, &key, None).is_ok()
                        && sess.authenticated()
                    {
                        sess.set_compress(true);
                        sess.set_keepalive(true, keepalive);
                        let fs =
                            crate::fs::fs_sftp::SftpFs::new(sess, host.clone(), user.clone(), None);
                        return Ok((session_id.clone(), fs));
                    }
                }

                if agent_connected && agent_identity_count > 0 {
                    Err(SshError::Auth(AuthError::AgentError(
                        "Agent authentication failed".to_string(),
                    )))
                } else {
                    Err(SshError::Auth(AuthError::KeyAuthFailed))
                }
            },
            grace,
            hard,
        )
        .await
        .map_err(|e| SshError::Internal(format!("Connection error: {e:?}")))?
    }

    /// Windows variant, uses russh.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or authentication with any found key fails.
    #[cfg(windows)]
    pub async fn try_connect_with_keys(
        &self,
        host: String,
        port: u16,
        user: String,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let session_id = Self::generate_session_id(&host, port);
        let timeout = Duration::from_secs(self.watchdog_secs);
        let fs = tokio::time::timeout(
            timeout,
            crate::fs::fs_sftp_russh::SftpFs::connect_pubkey(&host, port, &user),
        )
        .await
        .map_err(|_| SshError::Network(NetworkError::ConnectionTimedOut))?
        .map_err(|_e| SshError::Auth(AuthError::KeyAuthFailed))?;
        Ok((session_id, fs))
    }

    #[cfg(unix)]
    fn find_default_ssh_keys() -> Vec<PathBuf> {
        let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
        let ssh_dir = home.join(".ssh");
        if !ssh_dir.exists() || !ssh_dir.is_dir() {
            return Vec::new();
        }

        let key_files = [
            "id_ed25519",
            "id_ecdsa",
            "id_rsa",
            "id_ed25519_sk",
            "id_ecdsa_sk",
            "id_rsa_sk",
        ];
        key_files
            .iter()
            .filter(|&f| ssh_dir.join(f).exists())
            .map(|f| ssh_dir.join(f))
            .collect()
    }

    #[must_use]
    pub fn compute_backoff(&self, attempt: u32) -> Duration {
        let attempt = attempt.max(1);
        let mut secs =
            self.base_backoff.as_secs_f64() * self.backoff_factor.powf(f64::from(attempt - 1));
        if secs > self.max_backoff.as_secs_f64() {
            secs = self.max_backoff.as_secs_f64();
        }

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let seed = f64::from(u16::try_from(i64::from(nanos) % 1000).unwrap_or(0)) / 1000.0; // 0..1
        let jitter = 1.0 + (seed * 2.0 - 1.0) * self.jitter_pct;
        secs *= jitter;
        if secs < 0.0 {
            secs = 0.0;
        }
        Duration::from_secs_f64(secs)
    }

    #[must_use]
    pub fn generate_session_id(host: &str, port: u16) -> String {
        use std::time::SystemTime;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("ssh_{host}_{port}_{now}")
    }

    /// Connects to a remote host via SSH.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    #[cfg(unix)]
    pub async fn connect_ssh(
        &self,
        host: String,
        port: u16,
        user: String,
        password: String,
        _target_path: Option<String>,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let addr = format!("{host}:{port}");
        let tcp = tokio::net::TcpStream::connect(&addr)
            .await
            .map_err(|e| SshError::Network(e.into()))?;
        let tcp = tcp
            .into_std()
            .map_err(|e| SshError::Internal(e.to_string()))?;

        let session_id = Self::generate_session_id(&host, port);

        let grace = Duration::from_secs(self.read_timeout_secs);
        let hard = Duration::from_secs(self.watchdog_secs);

        let keepalive = self.keepalive_interval;
        let host_clone = host.clone();
        let user_clone = user.clone();

        self.spawn_blocking_with_watchdog(
            move || -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
                let mut sess =
                    ssh2::Session::new().map_err(|e| SshError::Internal(e.to_string()))?;
                sess.set_tcp_stream(tcp);
                sess.handshake()
                    .map_err(|e| {
                        let mut msg = e.to_string();
                        if matches!(e.code(), ssh2::ErrorCode::Session(-5)) {
                            msg.push_str(" (Check if your server requires modern SHA-2 RSA or Curve25519; ensure you're using ssh2 0.9.5+)");
                        }
                        SshError::Network(NetworkError::Other(msg))
                    })?;
                sess.userauth_password(&user_clone, &password)
                    .map_err(|_| SshError::Auth(AuthError::PasswordAuthFailed))?;
                if !sess.authenticated() {
                    return Err(SshError::Auth(AuthError::PasswordAuthFailed));
                }
                sess.set_compress(true);
                sess.set_keepalive(true, keepalive);
                let fs =
                    crate::fs::fs_sftp::SftpFs::new(sess, host_clone, user_clone, Some(password));
                Ok((session_id.clone(), fs))
            },
            grace,
            hard,
        )
        .await
        .map_err(|e| SshError::Internal(format!("Connection failed: {e:?}")))?
    }

    /// Windows variant, uses russh.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection or password authentication fails.
    #[cfg(windows)]
    pub async fn connect_ssh(
        &self,
        host: String,
        port: u16,
        user: String,
        password: String,
        _target_path: Option<String>,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let session_id = Self::generate_session_id(&host, port);
        let timeout = Duration::from_secs(self.watchdog_secs);
        let fs = tokio::time::timeout(
            timeout,
            crate::fs::fs_sftp_russh::SftpFs::connect_password(&host, port, &user, &password),
        )
        .await
        .map_err(|_| SshError::Network(NetworkError::ConnectionTimedOut))?
        .map_err(|_e| SshError::Auth(AuthError::PasswordAuthFailed))?;
        Ok((session_id, fs))
    }

    /// Registers a new SSH session.
    ///
    /// # Panics
    ///
    /// Panics if the session mutex cannot be locked.
    pub fn register_session(
        &self,
        session_id: String,
        host: String,
        port: u16,
        user: String,
        target_path: Option<String>,
    ) {
        let state = SessionState {
            session_id: session_id.clone(),
            host,
            port,
            user,
            target_path,
        };
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(session_id, state);
    }

    /// Unregisters an SSH session.
    ///
    /// # Panics
    ///
    /// Panics if the session mutex cannot be locked.
    pub fn unregister_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(session_id);
    }

    /// Gets a session by ID.
    ///
    /// # Panics
    ///
    /// Panics if the session mutex cannot be locked.
    #[must_use]
    pub fn get_session(&self, session_id: &str) -> Option<SessionState> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(session_id).cloned()
    }

    /// Gets all sessions.
    ///
    /// # Panics
    ///
    /// Panics if the session mutex cannot be locked.
    #[must_use]
    pub fn get_all_sessions(&self) -> Vec<SessionState> {
        let sessions = self.sessions.read().unwrap();
        sessions.values().cloned().collect()
    }

    /// Caches a password for a session.
    ///
    /// # Panics
    ///
    /// Panics if the password cache mutex cannot be locked.
    pub fn cache_password(&self, session_id: &str, password: String) {
        let mut cache = self.password_cache.write().unwrap();
        cache.insert(session_id.to_string(), password);
    }

    /// Gets a cached password for a session.
    ///
    /// # Panics
    ///
    /// Panics if the password cache mutex cannot be locked.
    #[must_use]
    pub fn get_cached_password(&self, session_id: &str) -> Option<String> {
        let cache = self.password_cache.read().unwrap();
        cache.get(session_id).cloned()
    }

    /// Clears a cached password for a session.
    ///
    /// # Panics
    ///
    /// Panics if the password cache mutex cannot be locked.
    pub fn clear_password(&self, session_id: &str) {
        let mut cache = self.password_cache.write().unwrap();
        cache.remove(session_id);
    }

    /// Clears all cached passwords.
    ///
    /// # Panics
    ///
    /// Panics if the password cache mutex cannot be locked.
    pub fn clear_all_passwords(&self) {
        let mut cache = self.password_cache.write().unwrap();
        cache.clear();
    }

    /// Reconnects a session with a new password.
    ///
    /// # Errors
    ///
    /// Returns an error if reconnection fails.
    pub async fn reconnect_session(
        &self,
        session_id: &str,
        password: String,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let session = self.get_session(session_id).ok_or_else(|| {
            SshError::InvalidInput(format!("Session {session_id} not found for reconnection"))
        })?;

        let (new_session_id, fs) = self
            .reconnect_with_backoff(
                session.host.clone(),
                session.port,
                session.user.clone(),
                password,
                session.target_path.clone(),
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
        );

        Ok((new_session_id, fs))
    }

    /// Reconnects with exponential backoff.
    ///
    /// # Errors
    ///
    /// Returns an error if reconnection fails after all attempts.
    pub async fn reconnect_with_backoff(
        &self,
        host: String,
        port: u16,
        user: String,
        password: String,
        target_path: Option<String>,
        max_attempts: Option<u32>,
    ) -> Result<(String, crate::fs::fs_sftp::SftpFs), SshError> {
        let mut attempt = 1u32;
        let max_attempts = max_attempts.unwrap_or(u32::MAX);

        loop {
            match self
                .connect_ssh(
                    host.clone(),
                    port,
                    user.clone(),
                    password.clone(),
                    target_path.clone(),
                )
                .await
            {
                Ok(result) => return Ok(result),
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

    /// Spawns a blocking operation with a watchdog timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation times out or fails.
    pub async fn spawn_blocking_with_watchdog<T, F>(
        &self,
        f: F,
        grace: Duration,
        hard: Duration,
    ) -> Result<T, SshManagerError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let mut jh = tokio::task::spawn_blocking(f);

        tokio::select! {
            join_res = &mut jh => {
                match join_res {
                    Ok(v) => Ok(v),
                    Err(e) => Err(SshManagerError::JoinError(e)),
                }
            }
            () = tokio::time::sleep(grace) => {
                // Grace exceeded; attempt abort and wait for hard duration
                jh.abort();
                tokio::select! {
                    join_res = &mut jh => {
                        match join_res {
                            Ok(v) => Ok(v),
                            Err(e) => Err(SshManagerError::JoinError(e)),
                        }
                    }
                    () = tokio::time::sleep(hard) => {
                        Err(SshManagerError::HardKill)
                    }
                }
            }
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

    #[test]
    fn test_backoff_values_and_jitter_bounds() {
        let mgr = SshManager {
            base_backoff: Duration::from_secs(1),
            backoff_factor: 2.0,
            max_backoff: Duration::from_secs(60),
            jitter_pct: 0.0,
            keepalive_interval: 10,
            read_timeout_secs: 15,
            watchdog_secs: 30,
            sessions: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            password_cache: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
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

    #[tokio::test]
    async fn test_spawn_blocking_with_watchdog_success() {
        let mgr = SshManager::new(None);

        let res = mgr
            .spawn_blocking_with_watchdog(
                || {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    123
                },
                Duration::from_millis(100),
                Duration::from_millis(100),
            )
            .await;

        assert_eq!(res.unwrap(), 123);
    }

    #[tokio::test]
    async fn test_spawn_blocking_with_watchdog_hard_kill() {
        let mgr = SshManager::new(None);

        let res = mgr
            .spawn_blocking_with_watchdog(
                || {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    5
                },
                Duration::from_millis(10),
                Duration::from_millis(10),
            )
            .await;

        assert!(matches!(res, Err(SshManagerError::HardKill)));
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
        );

        // Verify session is registered
        let session = mgr.get_session("session_test");
        assert!(session.is_some());
        let session = session.unwrap();
        assert_eq!(session.host, "example.com");
        assert_eq!(session.port, 22);
        assert_eq!(session.user, "user");

        // Unregister session
        mgr.unregister_session("session_test");
        assert!(mgr.get_session("session_test").is_none());
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
        );
        mgr.register_session(
            "s2".to_string(),
            "host2.com".to_string(),
            22,
            "u2".to_string(),
            None,
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

        // Cache password
        mgr.cache_password(session_id, password.to_string());
        assert_eq!(
            mgr.get_cached_password(session_id),
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
        mgr.cache_password(session_id2, password2.to_string());
        assert_eq!(
            mgr.get_cached_password(session_id2),
            Some(password2.to_string())
        );

        // Clear password for session_id2
        mgr.clear_password(session_id2);
        assert!(mgr.get_cached_password(session_id2).is_none());

        // Cache password
        mgr.cache_password(session_id, password.to_string());
        assert_eq!(
            mgr.get_cached_password(session_id),
            Some(password.to_string())
        );

        // Clear password
        mgr.clear_password(session_id);
        assert!(mgr.get_cached_password(session_id).is_none());

        // Test multiple sessions
        mgr.cache_password("session1", "pass1".to_string());
        mgr.cache_password("session2", "pass2".to_string());
        assert_eq!(
            mgr.get_cached_password("session1"),
            Some("pass1".to_string())
        );
        assert_eq!(
            mgr.get_cached_password("session2"),
            Some("pass2".to_string())
        );

        // Clear all passwords
        mgr.clear_all_passwords();
        assert!(mgr.get_cached_password("session1").is_none());
        assert!(mgr.get_cached_password("session2").is_none());
    }
}
