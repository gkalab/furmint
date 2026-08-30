use russh::keys::ssh_key;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

/// Returns the `known_hosts` path: `~/.ssh/known_hosts` on all platforms.
/// On Windows `~` is the user profile directory (same file OpenSSH uses).
#[must_use]
pub fn known_hosts_path() -> Option<PathBuf> {
    #[allow(deprecated)]
    std::env::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

/// Host-key checker passed to the russh handler. Stores the presented key
/// when the host is unknown or the key has changed so the caller can
/// surface a TOFU prompt.
pub struct HostKeyChecker {
    pub host: String,
    pub port: u16,
    pub known: std::sync::Arc<KnownHosts>,
    pub presented: Mutex<Option<PresentedKey>>,
}

#[derive(Debug, Clone)]
pub struct PresentedKey {
    pub fingerprint: String,
    pub key_line: String,
    /// Fingerprint of the previously stored key, if any (`KeyChanged` case).
    pub stored_fp: Option<String>,
}

impl HostKeyChecker {
    #[must_use]
    pub fn new(host: String, port: u16, known: std::sync::Arc<KnownHosts>) -> Self {
        Self {
            host,
            port,
            known,
            presented: Mutex::new(None),
        }
    }

    pub fn take_presented(&self) -> Option<PresentedKey> {
        self.presented.lock().ok().and_then(|mut g| g.take())
    }
}

/// In-memory cache of `known_hosts` plus the file path for appends.
/// Uses interior locking so `Arc<KnownHosts>` can be shared across tasks.
pub struct KnownHosts {
    path: PathBuf,
    entries: RwLock<HashMap<String, String>>,
}

impl KnownHosts {
    #[must_use]
    pub fn new() -> Self {
        let path = known_hosts_path().unwrap_or_else(|| PathBuf::from("known_hosts"));
        let entries = Self::parse_file(&path).unwrap_or_default();
        Self {
            path,
            entries: RwLock::new(entries),
        }
    }

    /// For tests: create with an explicit file path.
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        let entries = Self::parse_file(&path).unwrap_or_default();
        Self {
            path,
            entries: RwLock::new(entries),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Host key used in the `HashMap`: `host` for port 22, `[host]:port` otherwise.
    #[must_use]
    pub fn host_key(host: &str, port: u16) -> String {
        if port == 22 {
            host.to_string()
        } else {
            format!("[{host}]:{port}")
        }
    }

    /// Parse file into `host_key -> "keytype base64"` map.
    /// Skips comments, empty lines, hashed `|1|` entries, and corrupt lines fail-soft per-entry.
    fn parse_file(path: &Path) -> anyhow::Result<HashMap<String, String>> {
        let mut map = HashMap::new();
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(map),
            Err(e) => return Err(e.into()),
        };
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Split into hosts, keytype, base64
            let mut parts = trimmed.split_whitespace();
            let Some(hosts) = parts.next() else {
                continue;
            };
            if hosts.starts_with("|1|") {
                continue;
            }
            let Some(keytype) = parts.next() else {
                continue;
            };
            let Some(b64) = parts.next() else {
                continue;
            };
            let key_line = format!("{keytype} {b64}");
            // Validate that the key parses
            if key_line_to_pubkey(&key_line).is_none() {
                continue;
            }
            for host_pat in hosts.split(',') {
                let host_pat = host_pat.trim();
                if host_pat.is_empty() || host_pat.starts_with("|1|") {
                    continue;
                }
                // Insert if not already present; first occurrence wins (consistent with OpenSSH)
                map.entry(host_pat.to_string())
                    .or_insert_with(|| key_line.clone());
            }
        }
        Ok(map)
    }

    /// Find stored key line for host/port. Returns the `"keytype base64"` string.
    #[must_use]
    pub fn find(&self, host: &str, port: u16) -> Option<String> {
        let key = Self::host_key(host, port);
        self.entries.read().ok()?.get(&key).cloned()
    }

    /// Fingerprint of the stored key for display (e.g. `SHA256:...`), if any.
    #[must_use]
    pub fn fingerprint_for(&self, host: &str, port: u16) -> Option<String> {
        let line = self.find(host, port)?;
        let pk = key_line_to_pubkey(&line)?;
        Some(pk.fingerprint(ssh_key::HashAlg::Sha256).to_string())
    }

    /// Reload the in-memory map from the file. Used before each connection
    /// attempt so external modifications are taken into account.
    pub fn reload(&self) {
        if let Ok(new) = Self::parse_file(&self.path)
            && let Ok(mut map) = self.entries.write()
        {
            *map = new;
        }
    }

    /// Insert or replace a host key in the file and update the in-memory map.
    /// If the host already exists, its old entry is removed first (replacing
    /// rather than appending duplicates). Creates parent dir if needed; sets
    /// 0600 on Unix.
    ///
    /// # Errors
    /// Returns an error if the file cannot be written.
    pub fn append(&self, host: &str, port: u16, key_line: &str) -> anyhow::Result<()> {
        // Validate key_line parses
        if key_line_to_pubkey(key_line).is_none() {
            anyhow::bail!("invalid key_line: {key_line}");
        }
        let host_key = Self::host_key(host, port);
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Read existing lines and filter out any old entry for this host_key.
        let mut kept_lines: Vec<String> = Vec::new();
        if let Ok(file) = File::open(&self.path) {
            let reader = BufReader::new(file);
            for line_res in reader.lines() {
                let Ok(line) = line_res else { continue };
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    kept_lines.push(line);
                    continue;
                }
                // Split hosts part (first whitespace token)
                let mut parts = trimmed.split_whitespace();
                let Some(hosts_part) = parts.next() else {
                    kept_lines.push(line);
                    continue;
                };
                if hosts_part.starts_with("|1|") {
                    // Hashed entry: keep as-is
                    kept_lines.push(line);
                    continue;
                }
                // Check if this line contains the target host_key
                let contains_target = hosts_part.split(',').any(|h| str::trim(h) == host_key);
                if contains_target {
                    // If hosts is a comma list, keep remaining hosts if any
                    if hosts_part.contains(',') {
                        let remaining: Vec<&str> = hosts_part
                            .split(',')
                            .map(str::trim)
                            .filter(|h| *h != host_key && !h.is_empty())
                            .collect();
                        if !remaining.is_empty() {
                            // Reconstruct line with remaining hosts + rest of original line
                            // rest includes keytype, b64, comment...
                            let rest = trimmed[hosts_part.len()..].trim_start();
                            let new_line = format!("{} {}", remaining.join(","), rest);
                            kept_lines.push(new_line);
                        }
                        // else: all hosts were target -> drop line entirely
                    }
                    // else single-host match -> drop line (replace)
                } else {
                    kept_lines.push(line);
                }
            }
        }

        // Rewrite file with kept lines plus new entry
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.path)?;
        for l in &kept_lines {
            file.write_all(l.as_bytes())?;
            file.write_all(b"\n")?;
        }
        file.write_all(host_key.as_bytes())?;
        file.write_all(b" ")?;
        file.write_all(key_line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        drop(file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }

        if let Ok(mut map) = self.entries.write() {
            map.insert(host_key, key_line.to_string());
        }
        Ok(())
    }
}

impl Default for KnownHosts {
    fn default() -> Self {
        Self::new()
    }
}

fn key_line_to_pubkey(line: &str) -> Option<ssh_key::PublicKey> {
    // line is expected to be "keytype base64"
    // Try OpenSSH parsing first; fallback to base64-only
    if let Ok(pk) = ssh_key::PublicKey::from_openssh(line) {
        return Some(pk);
    }
    let mut parts = line.split_whitespace();
    let _keytype = parts.next()?;
    let b64 = parts.next()?;
    russh::keys::parse_public_key_base64(b64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_key_line() -> String {
        // ssh-ed25519 known test vector from russh known_hosts tests
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ"
            .to_string()
    }

    fn sample_key2_line() -> String {
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G2sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X"
            .to_string()
    }

    #[test]
    fn test_parse_and_find() {
        let mut tmp = NamedTempFile::new().unwrap();
        let key1 = sample_key_line();
        let key2 = sample_key2_line();
        writeln!(tmp, "# comment").unwrap();
        writeln!(tmp, "example.com {key1}").unwrap();
        writeln!(tmp, "[example.com]:2222 {key2}").unwrap();
        writeln!(tmp, "|1|salt|hash {key1}").unwrap(); // should be skipped
        writeln!(tmp, "hosta,hostb {key1}").unwrap();
        writeln!(tmp).unwrap();
        tmp.flush().unwrap();
        let kh = KnownHosts::with_path(tmp.path().to_path_buf());
        assert_eq!(kh.find("example.com", 22), Some(key1.clone()));
        assert_eq!(kh.find("example.com", 2222), Some(key2.clone()));
        assert_eq!(kh.find("unknown", 22), None);
        // comma list
        assert_eq!(kh.find("hosta", 22), Some(key1.clone()));
        assert_eq!(kh.find("hostb", 22), Some(key1.clone()));
        // hashed skipped
        // fingerprint_for
        assert!(
            kh.fingerprint_for("example.com", 22)
                .unwrap()
                .starts_with("SHA256:")
        );
    }

    #[test]
    fn test_append_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let kh = KnownHosts::with_path(path.clone());
        let key1 = sample_key_line();
        kh.append("newhost.example", 22, &key1).unwrap();
        assert_eq!(kh.find("newhost.example", 22), Some(key1.clone()));

        let key2 = sample_key2_line();
        kh.append("other.example", 2222, &key2).unwrap();
        assert_eq!(kh.find("other.example", 2222), Some(key2.clone()));
        // Verify file contains both entries
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("newhost.example"));
        assert!(content.contains("[other.example]:2222"));

        // Reload from file
        let kh2 = KnownHosts::with_path(path);
        assert_eq!(kh2.find("newhost.example", 22), Some(key1));
        assert_eq!(kh2.find("other.example", 2222), Some(key2));
    }

    #[test]
    fn test_append_replaces_changed_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let kh = KnownHosts::with_path(path.clone());
        let key1 = sample_key_line();
        let key2 = sample_key2_line();
        kh.append("example.com", 22, &key1).unwrap();
        assert_eq!(kh.find("example.com", 22), Some(key1.clone()));
        // Replace with different key
        kh.append("example.com", 22, &key2).unwrap();
        assert_eq!(kh.find("example.com", 22), Some(key2.clone()));
        let content = std::fs::read_to_string(&path).unwrap();
        // Should contain exactly one entry for example.com, with key2 not key1
        let count = content.matches("example.com").count();
        assert_eq!(count, 1, "should replace, not duplicate: {content}");
        assert!(content.contains(&key2));
        assert!(!content.contains(&key1) || content.matches(&key1).count() == 0);
        // Reload should still find key2
        let kh2 = KnownHosts::with_path(path);
        assert_eq!(kh2.find("example.com", 22), Some(key2));
    }

    #[test]
    fn test_reload_picks_up_external_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let key1 = sample_key_line();
        let key2 = sample_key2_line();
        // Create file with key1
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "example.com {key1}").unwrap();
        }
        let kh = KnownHosts::with_path(path.clone());
        assert_eq!(kh.find("example.com", 22), Some(key1.clone()));
        // Externally change file to key2
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "example.com {key2}").unwrap();
        }
        // Before reload, still old
        assert_eq!(kh.find("example.com", 22), Some(key1.clone()));
        kh.reload();
        assert_eq!(kh.find("example.com", 22), Some(key2.clone()));
    }

    #[test]
    fn test_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent_known_hosts");
        let kh = KnownHosts::with_path(path);
        assert_eq!(kh.find("any", 22), None);
    }

    #[test]
    fn test_hashed_lines_skipped() {
        let mut tmp = NamedTempFile::new().unwrap();
        let key1 = sample_key_line();
        writeln!(
            tmp,
            "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= {key1}"
        )
        .unwrap();
        tmp.flush().unwrap();
        let kh = KnownHosts::with_path(tmp.path().to_path_buf());
        assert_eq!(kh.find("example.com", 22), None);
    }

    #[test]
    fn test_known_hosts_path_uses_home() {
        // Just ensure the function returns something ending with .ssh/known_hosts
        if let Some(p) = known_hosts_path() {
            assert!(p.ends_with(".ssh/known_hosts") || p.ends_with(".ssh\\known_hosts"));
        }
    }
}
