//! Error types reported by the SFTP backend.
//!
//! They are re-exported from [`super`], so callers keep using
//! `crate::fs::sftp::HostKeyMismatch` and friends.

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
