use crate::config::SshConfig;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Operation {
    pub id: u64,
    pub kind: String,
    pub path: String,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionState {
    pub session_id: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub target_path: Option<String>,
    pub is_connected: bool,
    pub reconnect_attempts: u32,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum SshManagerError {
    #[allow(dead_code)]
    JoinError(tokio::task::JoinError),
    HardKill,
}

#[derive(Clone)]
pub struct SshManager {
    base_dir: PathBuf,
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
    pub fn new(base_dir: Option<PathBuf>, ssh_config: Option<&SshConfig>) -> Self {
        let base = if let Some(p) = base_dir {
            p
        } else {
            ProjectDirs::from("org", "fm", "fm")
                .map(|d| d.data_dir().to_path_buf())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        };

        let keepalive_interval = ssh_config.and_then(|c| c.keepalive_interval).unwrap_or(10);
        let read_timeout_secs = ssh_config.and_then(|c| c.read_timeout_secs).unwrap_or(15);
        let watchdog_secs = ssh_config.and_then(|c| c.watchdog_secs).unwrap_or(30);

        Self {
            base_dir: base,
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

    #[allow(dead_code)]
    pub fn with_params(
        base_dir: Option<PathBuf>,
        ssh_config: Option<&SshConfig>,
        base_backoff: Duration,
        backoff_factor: f64,
        max_backoff: Duration,
        jitter_pct: f64,
    ) -> Self {
        let mut s = Self::new(base_dir, ssh_config);
        s.base_backoff = base_backoff;
        s.backoff_factor = backoff_factor;
        s.max_backoff = max_backoff;
        s.jitter_pct = jitter_pct;
        s
    }

    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        let mut p = self.base_dir.clone();
        p.push("ssh");
        p.push("sessions");
        p.push(session_id);
        p
    }

    #[allow(dead_code)]
    pub fn enqueue_op(&self, session_id: &str, op: Operation) -> anyhow::Result<()> {
        let dir = self.session_dir(session_id);
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("queue.json");

        let mut ops: Vec<Operation> = if file.exists() {
            let content = std::fs::read_to_string(&file)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        ops.push(op);
        let content = serde_json::to_string_pretty(&ops)?;
        std::fs::write(&file, content)?;
        Ok(())
    }

    pub fn clear_queue(&self, session_id: &str) -> anyhow::Result<()> {
        let file = self.session_dir(session_id).join("queue.json");
        if file.exists() {
            std::fs::remove_file(file)?;
        }
        Ok(())
    }

    pub fn read_queue(&self, session_id: &str) -> anyhow::Result<Vec<Operation>> {
        let file = self.session_dir(session_id).join("queue.json");
        if !file.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&file)?;
        let ops: Vec<Operation> = serde_json::from_str(&content).unwrap_or_default();
        Ok(ops)
    }

    pub fn compute_backoff(&self, attempt: u32) -> Duration {
        let attempt = attempt.max(1);
        let mut secs =
            self.base_backoff.as_secs_f64() * self.backoff_factor.powf((attempt - 1) as f64);
        if secs > self.max_backoff.as_secs_f64() {
            secs = self.max_backoff.as_secs_f64();
        }

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let seed = (nanos as i64 % 1000) as f64 / 1000.0; // 0..1
        let jitter = 1.0 + (seed * 2.0 - 1.0) * self.jitter_pct;
        secs *= jitter;
        if secs < 0.0 {
            secs = 0.0;
        }
        Duration::from_secs_f64(secs)
    }

    pub async fn replay_queue<F, Fut>(
        &self,
        session_id: &str,
        mut executor: F,
    ) -> Result<(), anyhow::Error>
    where
        F: FnMut(Operation) -> Fut,
        Fut: std::future::Future<Output = Result<(), anyhow::Error>>,
    {
        let ops = self.read_queue(session_id)?;
        for op in ops {
            executor(op).await?;
        }
        // Clear queue after successful replay
        self.clear_queue(session_id)?;
        Ok(())
    }

    pub fn generate_session_id(&self, host: &str, port: u16) -> String {
        use std::time::SystemTime;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("ssh_{}_{}_{}", host, port, now)
    }

    pub async fn connect_ssh(
        &self,
        host: String,
        port: u16,
        user: String,
        password: String,
        _target_path: Option<String>,
    ) -> Result<(String, crate::fs_sftp::SftpFs), anyhow::Error> {
        let session_id = self.generate_session_id(&host, port);

        let grace = Duration::from_secs(self.read_timeout_secs);
        let hard = Duration::from_secs(self.watchdog_secs);

        // Clone values needed by the blocking closure to avoid borrowing self
        let keepalive = self.keepalive_interval;
        let host_clone = host.clone();
        let user_clone = user.clone();

        self.spawn_blocking_with_watchdog(
            move || -> anyhow::Result<(String, crate::fs_sftp::SftpFs)> {
                use std::net::TcpStream;
                let tcp = TcpStream::connect(format!("{}:{}", host_clone, port))?;
                let mut sess = ssh2::Session::new()?;
                sess.set_tcp_stream(tcp);
                sess.handshake()?;
                sess.userauth_password(&user_clone, &password)?;
                if !sess.authenticated() {
                    return Err(anyhow::anyhow!("Authentication failed"));
                }
                // Set keepalive interval
                sess.set_keepalive(true, keepalive);
                let fs = crate::fs_sftp::SftpFs::new(sess, host_clone, user_clone);
                Ok((session_id.clone(), fs))
            },
            grace,
            hard,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Connection failed: {:?}", e))?
    }

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
            is_connected: true,
            reconnect_attempts: 0,
        };
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(session_id, state);
    }

    pub fn unregister_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(session_id);
    }

    #[allow(dead_code)]
    pub fn mark_disconnected(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(state) = sessions.get_mut(session_id) {
            state.is_connected = false;
        }
    }

    pub fn get_session(&self, session_id: &str) -> Option<SessionState> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(session_id).cloned()
    }

    pub fn get_all_sessions(&self) -> Vec<SessionState> {
        let sessions = self.sessions.read().unwrap();
        sessions.values().cloned().collect()
    }

    #[allow(dead_code)]
    pub fn has_pending_operations(&self, session_id: &str) -> bool {
        self.read_queue(session_id)
            .map(|q| !q.is_empty())
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn get_pending_operation_count(&self, session_id: &str) -> usize {
        self.read_queue(session_id).map(|q| q.len()).unwrap_or(0)
    }

    pub fn cache_password(&self, session_id: &str, password: String) {
        let mut cache = self.password_cache.write().unwrap();
        cache.insert(session_id.to_string(), password);
    }

    #[allow(dead_code)]
    pub fn get_cached_password(&self, session_id: &str) -> Option<String> {
        let cache = self.password_cache.read().unwrap();
        cache.get(session_id).cloned()
    }

    pub fn clear_password(&self, session_id: &str) {
        let mut cache = self.password_cache.write().unwrap();
        cache.remove(session_id);
    }

    pub fn clear_all_passwords(&self) {
        let mut cache = self.password_cache.write().unwrap();
        cache.clear();
    }

    pub async fn reconnect_session<F, Fut>(
        &self,
        session_id: &str,
        password: String,
        mut executor: F,
    ) -> Result<(String, crate::fs_sftp::SftpFs), anyhow::Error>
    where
        F: FnMut(Operation) -> Fut,
        Fut: std::future::Future<Output = Result<(), anyhow::Error>>,
    {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow::anyhow!("Session {} not found for reconnection", session_id))?;

        // Attempt reconnection with backoff - limit attempts for manual reconnect
        let (new_session_id, fs) = self
            .reconnect_with_backoff(
                session.host.clone(),
                session.port,
                session.user.clone(),
                password,
                session.target_path.clone(),
                Some(3), // Limit manual reconnect attempts
            )
            .await?;

        // Replay queued operations
        self.replay_queue(&new_session_id, &mut executor).await?;

        // Update session tracking
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

    pub async fn reconnect_with_backoff(
        &self,
        host: String,
        port: u16,
        user: String,
        password: String,
        target_path: Option<String>,
        max_attempts: Option<u32>,
    ) -> Result<(String, crate::fs_sftp::SftpFs), anyhow::Error> {
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
                Err(e) => {
                    if attempt >= max_attempts {
                        return Err(anyhow::anyhow!(
                            "Reconnection failed after {} attempts: {}",
                            attempt,
                            e
                        ));
                    }
                    let delay = self.compute_backoff(attempt);
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

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
            _ = tokio::time::sleep(grace) => {
                // Grace exceeded; attempt abort and wait for hard duration
                jh.abort();
                tokio::select! {
                    join_res = &mut jh => {
                        match join_res {
                            Ok(v) => Ok(v),
                            Err(e) => Err(SshManagerError::JoinError(e)),
                        }
                    }
                    _ = tokio::time::sleep(hard) => {
                        Err(SshManagerError::HardKill)
                    }
                }
            }
        }
    }
}

impl Default for SshManager {
    fn default() -> Self {
        SshManager::new(None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_enqueue_and_read_queue() {
        let tmp = tempdir().unwrap();
        let mgr = SshManager::new(Some(tmp.path().to_path_buf()), None);

        let sid = "session_pending";
        assert!(!mgr.has_pending_operations(sid));
        assert_eq!(mgr.get_pending_operation_count(sid), 0);

        mgr.enqueue_op(
            sid,
            Operation {
                id: 1,
                kind: "test".to_string(),
                path: "/test".to_string(),
                payload: None,
            },
        )
        .unwrap();

        assert!(mgr.has_pending_operations(sid));
        assert_eq!(mgr.get_pending_operation_count(sid), 1);

        mgr.clear_queue(sid).unwrap();
        assert!(!mgr.has_pending_operations(sid));
        assert_eq!(mgr.get_pending_operation_count(sid), 0);
    }

    #[tokio::test]
    async fn test_backoff_values_and_jitter_bounds() {
        let tmp = tempdir().unwrap();
        let mgr = SshManager::with_params(
            Some(tmp.path().to_path_buf()),
            None,
            Duration::from_secs(1),
            2.0,
            Duration::from_secs(60),
            0.0,
        );

        assert_eq!(mgr.compute_backoff(1).as_secs(), 1);
        assert_eq!(mgr.compute_backoff(2).as_secs(), 2);
        assert_eq!(mgr.compute_backoff(3).as_secs(), 4);
        assert_eq!(mgr.compute_backoff(8).as_secs(), 60);

        let mgr_j = SshManager::with_params(
            Some(tmp.path().to_path_buf()),
            None,
            Duration::from_secs(1),
            2.0,
            Duration::from_secs(60),
            0.2,
        );

        let d = mgr_j.compute_backoff(3);
        let expected = 4.0;
        let low = (expected * (1.0 - 0.2)) as f64;
        let high = (expected * (1.0 + 0.2)) as f64;
        let got = d.as_secs_f64();
        assert!(
            got >= low && got <= high,
            "got {} not in [{}..{}]",
            got,
            low,
            high
        );
    }

    #[test]
    fn test_generate_session_id() {
        let mgr = SshManager::new(None, None);

        let id1 = mgr.generate_session_id("example.com", 22);
        assert!(id1.starts_with("ssh_example.com_22_"));
        // Verify it contains a timestamp-like suffix
        let suffix = &id1["ssh_example.com_22_".len()..];
        assert!(
            suffix.parse::<u64>().is_ok(),
            "suffix should be numeric: {}",
            suffix
        );
    }

    #[test]
    fn test_clear_queue() {
        let tmp = tempdir().unwrap();
        let mgr = SshManager::new(Some(tmp.path().to_path_buf()), None);

        let sid = "session_clear";
        let op = Operation {
            id: 1,
            kind: "test".to_string(),
            path: "/test".to_string(),
            payload: None,
        };
        mgr.enqueue_op(sid, op.clone()).unwrap();
        assert!(mgr.read_queue(sid).unwrap().len() > 0);

        mgr.clear_queue(sid).unwrap();
        assert_eq!(mgr.read_queue(sid).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_spawn_blocking_with_watchdog_success() {
        let tmp = tempdir().unwrap();
        let mgr = SshManager::new(Some(tmp.path().to_path_buf()), None);

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
        let tmp = tempdir().unwrap();
        let mgr = SshManager::new(Some(tmp.path().to_path_buf()), None);

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
        let mgr = SshManager::new(None, None);

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
        assert!(session.is_connected);

        // Mark as disconnected
        mgr.mark_disconnected("session_test");
        let session = mgr.get_session("session_test").unwrap();
        assert!(!session.is_connected);

        // Unregister session
        mgr.unregister_session("session_test");
        assert!(mgr.get_session("session_test").is_none());
    }

    #[test]
    fn test_get_all_sessions() {
        let mgr = SshManager::new(None, None);

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
    fn test_pending_operations() {
        let tmp = tempdir().unwrap();
        let mgr = SshManager::new(Some(tmp.path().to_path_buf()), None);

        let sid = "session_pending";
        assert!(!mgr.has_pending_operations(sid));
        assert_eq!(mgr.get_pending_operation_count(sid), 0);

        mgr.enqueue_op(
            sid,
            Operation {
                id: 1,
                kind: "test".to_string(),
                path: "/test".to_string(),
                payload: None,
            },
        )
        .unwrap();

        assert!(mgr.has_pending_operations(sid));
        assert_eq!(mgr.get_pending_operation_count(sid), 1);

        mgr.clear_queue(sid).unwrap();
        assert!(!mgr.has_pending_operations(sid));
        assert_eq!(mgr.get_pending_operation_count(sid), 0);
    }

    #[tokio::test]
    async fn test_replay_queue() {
        let tmp = tempdir().unwrap();
        let mgr = SshManager::new(Some(tmp.path().to_path_buf()), None);

        let sid = "session_replay";
        // Queue some operations
        mgr.enqueue_op(
            sid,
            Operation {
                id: 1,
                kind: "mkdir".to_string(),
                path: "/remote/dir1".to_string(),
                payload: None,
            },
        )
        .unwrap();
        mgr.enqueue_op(
            sid,
            Operation {
                id: 2,
                kind: "mkdir".to_string(),
                path: "/remote/dir2".to_string(),
                payload: None,
            },
        )
        .unwrap();

        // Track executed operations
        let executed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let executed_clone = executed.clone();

        let result = mgr
            .replay_queue(sid, move |op| {
                let mut ex = executed_clone.lock().unwrap();
                ex.push(op.kind.clone());
                Box::pin(async { Ok(()) })
            })
            .await;

        assert!(result.is_ok());
        let ex = executed.lock().unwrap();
        assert_eq!(ex.len(), 2);
        assert_eq!(ex[0], "mkdir");
        assert_eq!(ex[1], "mkdir");

        // Queue should be cleared after replay
        assert!(mgr.read_queue(sid).unwrap().is_empty());
    }

    #[test]
    fn test_password_cache() {
        let mgr = SshManager::new(None, None);

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
