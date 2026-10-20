use std::path::{Path, PathBuf};

/// Returns a list of default SSH private key paths found in the user's home directory.
#[must_use]
pub fn find_default_ssh_keys() -> Vec<PathBuf> {
    let home = std::env::home_dir().unwrap_or_else(|| {
        #[cfg(windows)]
        {
            PathBuf::from("C:\\Users\\default")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/root")
        }
    });
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

/// Standardizes a path for SFTP servers (forward slashes, leading slash, no double slashes, no trailing slash except root).
#[must_use]
pub fn normalize_sftp_path(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");

    // Ensure leading slash
    if !s.starts_with('/') {
        s = format!("/{s}");
    }

    // Remove double slashes
    while s.contains("//") {
        s = s.replace("//", "/");
    }

    // Remove trailing slash for non-root paths
    if s.len() > 1 && s.ends_with('/') {
        s.pop();
    }

    s
}
