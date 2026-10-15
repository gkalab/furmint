use super::*;
use std::io::Write;
use tempfile::NamedTempFile;

fn sample_key_line() -> String {
    // ssh-ed25519 known test vector from russh known_hosts tests
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ".to_string()
}

fn sample_key2_line() -> String {
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G2sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X".to_string()
}

/// Build a `|1|<salt>|<mac>` host field for `host` using `salt`.
fn hash_host(host: &str, salt: &[u8]) -> String {
    let mac = hmac_sha1(salt, host.as_bytes());
    format!("|1|{}|{}", B64.encode(salt), B64.encode(mac))
}

#[test]
fn test_parse_and_find() {
    let mut tmp = NamedTempFile::new().unwrap();
    let key1 = sample_key_line();
    let key2 = sample_key2_line();
    writeln!(tmp, "# comment").unwrap();
    writeln!(tmp, "example.com {key1}").unwrap();
    writeln!(tmp, "[example.com]:2222 {key2}").unwrap();
    writeln!(tmp, "|1|salt|hash {key1}").unwrap(); // hashed, host "unknown" -> not matched below
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
    let key1 = sample_key_line();
    let key2 = sample_key2_line();
    kh_append_example(&path, &key1);
    let kh = KnownHosts::with_path(path.clone());
    assert_eq!(kh.find("example.com", 22), Some(key1.clone()));
    // Replace with different key
    kh.append("example.com", 22, &key2).unwrap();
    assert_eq!(kh.find("example.com", 22), Some(key2.clone()));
    let content = std::fs::read_to_string(&path).unwrap();
    // Should contain exactly one entry for example.com, with key2 not key1
    let count = content.matches("example.com").count();
    assert_eq!(count, 1, "should replace, not duplicate: {content}");
    assert!(content.contains(&key2));
    assert_eq!(content.matches(&key1).count(), 0);
    // Reload should still find key2
    let kh2 = KnownHosts::with_path(path);
    assert_eq!(kh2.find("example.com", 22), Some(key2));
}

/// Helper: seed a `known_hosts` file with an example.com entry.
fn kh_append_example(path: &Path, key1: &str) {
    let kh = KnownHosts::with_path(path.to_path_buf());
    kh.append("example.com", 22, key1).unwrap();
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
fn test_hmac_round_trip_matches() {
    let salt = [0u8; 20];
    let mac = hmac_sha1(&salt, b"example.com");
    // The hash of example.com must verify back to example.com...
    let e = Entries {
        plain: HashMap::new(),
        hashed: vec![HashedEntry {
            salt_b64: B64.encode(salt),
            mac_b64: B64.encode(mac),
            key_line: sample_key_line(),
        }],
        uses_hashing: true,
    };
    let mut kh = KnownHosts::with_path(PathBuf::from("/nonexistent/kh"));
    *kh.entries.get_mut().unwrap() = e;
    assert_eq!(kh.find("example.com", 22), Some(sample_key_line()));
    // ...and must NOT match a different host.
    assert_eq!(kh.find("other.example", 22), None);
}

#[test]
fn test_find_matches_hashed_entry_in_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known_hosts");
    let key1 = sample_key_line();
    let salt = [7u8; 20];
    let hashed_field = hash_host("secret.example", &salt);
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "{hashed_field} {key1}").unwrap();
    drop(f);
    let kh = KnownHosts::with_path(path);
    assert_eq!(kh.find("secret.example", 22), Some(key1));
    assert_eq!(kh.find("notsecret.example", 22), None);
}

#[test]
fn test_append_to_hashed_file_writes_hashed_and_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known_hosts");
    let key1 = sample_key_line();
    let key2 = sample_key2_line();
    // Seed a hashed entry for example.com.
    let salt = [3u8; 20];
    let hashed_field = hash_host("example.com", &salt);
    {
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{hashed_field} {key1}").unwrap();
    }
    let kh = KnownHosts::with_path(path.clone());
    // Existing hashed entry is found.
    assert_eq!(kh.find("example.com", 22), Some(key1.clone()));
    // Replacing it should write a NEW hashed entry (style mirrored) and drop the old.
    kh.append("example.com", 22, &key2).unwrap();
    assert_eq!(kh.find("example.com", 22), Some(key2.clone()));
    let content = std::fs::read_to_string(&path).unwrap();
    // File must now contain a hashed entry (starts with |1|) and no plaintext example.com.
    assert!(
        content.lines().any(|l| l.starts_with("|1|")),
        "expected hashed entry: {content}"
    );
    assert!(
        !content.lines().any(|l| l.starts_with("example.com")),
        "old plaintext/old hashed must be gone: {content}"
    );
    // Reload: still finds key2, and the file is still marked hashed.
    let kh2 = KnownHosts::with_path(path);
    assert_eq!(kh2.find("example.com", 22), Some(key2));
}

#[test]
fn test_atomic_write_no_temp_left_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known_hosts");
    let kh = KnownHosts::with_path(path.clone());
    let key1 = sample_key_line();
    kh.append("a.example", 22, &key1).unwrap();
    let tmp = path.with_extension("known_hosts.tmp");
    assert!(!tmp.exists(), "temp file must be cleaned up");
    assert_eq!(kh.find("a.example", 22), Some(key1));
}

#[test]
fn test_known_hosts_path_uses_home() {
    // Just ensure the function returns something ending with .ssh/known_hosts
    if let Some(p) = known_hosts_path() {
        assert!(p.ends_with(".ssh/known_hosts") || p.ends_with(".ssh\\known_hosts"));
    }
}
