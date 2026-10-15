use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
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
    /// A diagnostic message set by the key-check handler when the presented
    /// server key could not be processed (e.g. failed to encode), so the
    /// connect flow can surface a clear error instead of failing silently.
    pub diag: Mutex<Option<String>>,
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
            diag: Mutex::new(None),
        }
    }

    pub fn take_presented(&self) -> Option<PresentedKey> {
        self.presented.lock().ok().and_then(|mut g| g.take())
    }

    /// Record a diagnostic (e.g. the server key could not be encoded).
    pub fn set_diag(&self, msg: String) {
        if let Ok(mut g) = self.diag.lock() {
            *g = Some(msg);
        }
    }

    /// Take a previously recorded diagnostic, if any.
    #[must_use]
    pub fn take_diag(&self) -> Option<String> {
        self.diag.lock().ok().and_then(|mut g| g.take())
    }
}

/// A hashed (`|1|`) `known_hosts` entry. The hostname is unrecoverable, so
/// membership is checked by recomputing `HMAC-SHA1(salt, host)`.
#[derive(Clone)]
struct HashedEntry {
    salt_b64: String,
    mac_b64: String,
    /// The `"keytype base64"` part of the entry.
    key_line: String,
}

/// Parsed contents of the `known_hosts` file.
#[derive(Default, Clone)]
struct Entries {
    /// Plaintext entries: `host_key -> "keytype base64"`.
    plain: HashMap<String, String>,
    /// Hashed (`|1|`) entries; matched by recomputing the HMAC.
    hashed: Vec<HashedEntry>,
    /// True if the file contains any hashed entry. New appends mirror this
    /// style so we don't silently downgrade a `HashKnownHosts` user's file.
    uses_hashing: bool,
}

/// In-memory cache of `known_hosts` plus the file path for appends.
/// Uses interior locking so `Arc<KnownHosts>` can be shared across tasks.
pub struct KnownHosts {
    path: PathBuf,
    entries: RwLock<Entries>,
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
    #[cfg(test)]
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        let entries = Self::parse_file(&path).unwrap_or_default();
        Self {
            path,
            entries: RwLock::new(entries),
        }
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

    /// Parse file into plaintext + hashed entry maps. Skips comments and
    /// empty lines; corrupt lines fail-soft per-entry.
    fn parse_file(path: &Path) -> anyhow::Result<Entries> {
        let mut plain: HashMap<String, String> = HashMap::new();
        let mut hashed: Vec<HashedEntry> = Vec::new();
        let mut uses_hashing = false;
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Entries {
                    plain,
                    hashed,
                    uses_hashing,
                });
            }
            Err(e) => return Err(e.into()),
        };
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Split into hosts, keytype, base64 (ignore any trailing comment)
            let mut parts = trimmed.split_whitespace();
            let Some(hosts) = parts.next() else {
                continue;
            };
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
            if hosts.starts_with("|1|") {
                let (salt, mac) = split_hashed_host(hosts);
                uses_hashing = true;
                hashed.push(HashedEntry {
                    salt_b64: salt.to_string(),
                    mac_b64: mac.to_string(),
                    key_line,
                });
                continue;
            }
            for host_pat in hosts.split(',') {
                let host_pat = host_pat.trim();
                if host_pat.is_empty() || host_pat.starts_with("|1|") {
                    continue;
                }
                // Insert if not already present; first occurrence wins (consistent with OpenSSH)
                plain
                    .entry(host_pat.to_string())
                    .or_insert_with(|| key_line.clone());
            }
        }
        Ok(Entries {
            plain,
            hashed,
            uses_hashing,
        })
    }

    /// Find stored key line for host/port. Returns the `"keytype base64"` string.
    /// Checks plaintext entries first, then hashed (`|1|`) entries by
    /// recomputing `HMAC-SHA1(salt, host)`.
    #[must_use]
    pub fn find(&self, host: &str, port: u16) -> Option<String> {
        let key = Self::host_key(host, port);
        let e = self.entries.read().ok()?;
        if let Some(kl) = e.plain.get(&key) {
            return Some(kl.clone());
        }
        for h in &e.hashed {
            let Ok(salt) = B64.decode(h.salt_b64.as_bytes()) else {
                continue;
            };
            if B64.encode(hmac_sha1(&salt, key.as_bytes())) == h.mac_b64 {
                return Some(h.key_line.clone());
            }
        }
        None
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
    /// If the host already exists (plaintext **or** hashed), its old entry is
    /// removed first (replacing rather than appending duplicates). New entries
    /// are written hashed when the file already uses hashing, matching the
    /// user's `HashKnownHosts` setting. The write is atomic: content is written
    /// to a temp file in the same directory and then renamed over the target,
    /// so a crash mid-write cannot corrupt the existing file. Creates parent
    /// dir if needed; sets 0600 on Unix.
    ///
    /// # Errors
    /// Returns an error if the key is invalid or the file cannot be written.
    pub fn append(&self, host: &str, port: u16, key_line: &str) -> anyhow::Result<()> {
        // Validate key_line parses
        if key_line_to_pubkey(key_line).is_none() {
            anyhow::bail!("invalid key_line: {key_line}");
        }
        let host_key = Self::host_key(host, port);
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Read existing lines, dropping any old entry (plaintext or hashed)
        // for this host_key so we can append a fresh one in its place.
        let kept_lines = self.read_lines_without_host(&host_key);

        // Choose new entry format: hashed if the file already uses hashing.
        let use_hash = self.entries.read().is_ok_and(|e| e.uses_hashing);
        let new_host_field = if use_hash {
            let salt: Vec<u8> = (0..20).map(|_| rand::random::<u8>()).collect();
            let mac = hmac_sha1(&salt, host_key.as_bytes());
            format!("|1|{}|{}", B64.encode(&salt), B64.encode(mac))
        } else {
            host_key.clone()
        };

        // Assemble the full new file content.
        let mut out = String::new();
        for l in &kept_lines {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str(&new_host_field);
        out.push(' ');
        out.push_str(key_line);
        out.push('\n');

        // Write atomically: temp file in the same directory, then rename over
        // the target so a crash mid-write never destroys the existing file.
        let tmp = self.path.with_extension("known_hosts.tmp");
        {
            let mut f = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            f.write_all(out.as_bytes())?;
            f.flush()?;
            f.sync_all()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        if let Err(e) = replace_file(&tmp, &self.path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }

        if let Ok(mut map) = self.entries.write() {
            map.plain.insert(host_key.clone(), key_line.to_string());
            // The new entry supersedes any hashed duplicate; drop it so stale
            // hashed lines for this host aren't matched on a later lookup.
            map.hashed.retain(|h| {
                !matches!(
                    B64.decode(h.salt_b64.as_bytes()),
                    Ok(salt_b) if B64.encode(hmac_sha1(&salt_b, host_key.as_bytes())) == h.mac_b64
                )
            });
        }
        Ok(())
    }

    /// Read the existing file and return all lines verbatim, except any entry
    /// (plaintext or hashed) whose host list contains `host_key`. Those are
    /// dropped (or, for a comma list, reduced to the non-target hosts) so the
    /// caller can append a fresh entry in their place.
    fn read_lines_without_host(&self, host_key: &str) -> Vec<String> {
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
                    let (salt, mac) = split_hashed_host(hosts_part);
                    let is_ours = matches!(
                        B64.decode(salt.as_bytes()),
                        Ok(salt_b) if B64.encode(hmac_sha1(&salt_b, host_key.as_bytes())) == mac
                    );
                    if is_ours {
                        // Drop; the new (re)hashed entry replaces it below.
                    } else {
                        kept_lines.push(line);
                    }
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
        kept_lines
    }
}

/// Cross-platform "move over an existing destination". On Windows a plain
/// rename fails when the destination exists, so use `MoveFileEx` with
/// `MOVEFILE_REPLACE_EXISTING`.
#[cfg(unix)]
fn replace_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::rename(src, dst)
}

#[cfg(windows)]
fn replace_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};
    use windows::core::PCWSTR;
    let src_w: Vec<u16> = src
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let dst_w: Vec<u16> = dst
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(src_w.as_ptr()),
            PCWSTR(dst_w.as_ptr()),
            MOVEFILE_REPLACE_EXISTING,
        )
    }?;
    Ok(())
}

/// Split a `|1|<salt>|<mac>` host field into its salt and mac components.
fn split_hashed_host(hosts: &str) -> (&str, &str) {
    let inner = hosts.strip_prefix("|1|").unwrap_or("");
    let mut it = inner.splitn(2, '|');
    (it.next().unwrap_or(""), it.next().unwrap_or(""))
}

/// OpenSSH legacy host-hash: `HMAC-SHA1(key=salt, msg=host)` (20-byte tag).
fn hmac_sha1(salt: &[u8], msg: &[u8]) -> [u8; 20] {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, salt);
    let tag = ring::hmac::sign(&key, msg);
    let mut out = [0u8; 20];
    out.copy_from_slice(tag.as_ref());
    out
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
mod tests;
