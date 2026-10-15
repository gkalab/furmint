use anyhow::Result;
use chrono::{DateTime, Local};
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Strips the Windows extended-length prefix returned by
/// `std::fs::canonicalize` (\\?\D:\...) so paths look like normal DOS paths.
/// UNC paths (\\?\UNC\server\share) are converted to \\server\share.
#[must_use]
#[cfg(target_os = "windows")]
pub fn strip_extended_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

#[must_use]
#[cfg(not(target_os = "windows"))]
pub fn strip_extended_prefix(path: PathBuf) -> PathBuf {
    path
}

/// Empties the user trash. Returns the number of removed items on supported
/// platforms (0 on Windows, where the API reports no count), or an error.
///
/// # Errors
///
/// Returns an error if the trash cannot be emptied.
pub async fn empty_trash() -> std::result::Result<usize, String> {
    tokio::task::spawn_blocking(empty_trash_sync)
        .await
        .map_err(|e| format!("Empty trash task error: {e}"))?
}

/// Blocking implementation of [`empty_trash`], meant to run off the runtime
/// worker threads (e.g. via `tokio::task::spawn_blocking`).
fn empty_trash_sync() -> std::result::Result<usize, String> {
    #[cfg(target_os = "windows")]
    {
        unsafe {
            use winapi::shared::windef::HWND;
            use winapi::shared::winerror::SUCCEEDED;
            use winapi::um::shellapi::{
                SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW,
            };

            let hwnd: HWND = std::ptr::null_mut();
            let psz_root: *const u16 = std::ptr::null(); // all drives

            let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
            let res = SHEmptyRecycleBinW(hwnd, psz_root, flags);

            // E_UNEXPECTED (0x8000ffff) is returned when the Recycle Bin is already empty.
            if SUCCEEDED(res) || res == winapi::shared::winerror::E_UNEXPECTED {
                Ok(0)
            } else {
                Err(format!("Failed: SHEmptyRecycleBinW error code {res:#x}"))
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::env;
        use std::fs;
        use std::path::PathBuf;
        let home = env::var("HOME").map_err(|e| format!("No HOME: {e}"))?;
        let trash_dir = PathBuf::from(format!("{}/.Trash", home));
        if !trash_dir.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in walkdir::WalkDir::new(&trash_dir).min_depth(1).max_depth(1) {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(path).map_err(|e| e.to_string())?;
            } else {
                fs::remove_file(path).map_err(|e| e.to_string())?;
            }
            removed += 1;
        }
        Ok(removed)
    }
    #[cfg(target_os = "linux")]
    {
        use std::env;
        use std::fs;
        use std::path::PathBuf;
        let home = env::var("HOME").map_err(|e| format!("No HOME: {e}"))?;
        let base = PathBuf::from(format!("{home}/.local/share/Trash"));
        let files = base.join("files");
        let info = base.join("info");
        let mut removed = 0;
        for dir in &[&files, &info] {
            if dir.exists() {
                for entry in walkdir::WalkDir::new(dir).min_depth(1).max_depth(1) {
                    let Ok(entry) = entry else { continue };
                    let path = entry.path();
                    if path.is_dir() {
                        fs::remove_dir_all(path).map_err(|e| e.to_string())?;
                    } else {
                        fs::remove_file(path).map_err(|e| e.to_string())?;
                    }
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err("Not supported on this OS".to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub attributes: String,
    pub selected: bool,
}

impl FileEntry {
    /// Creates a `FileEntry` from a directory entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry cannot be read.
    pub fn try_from_dir_entry(entry: &std::fs::DirEntry) -> Result<Self> {
        let file_type = entry.file_type()?;
        let meta = entry.metadata()?;

        let name = entry.file_name().to_string_lossy().to_string();
        let is_symlink = file_type.is_symlink();
        let mut is_dir = file_type.is_dir();

        // Windows reports whether a reparse point targets a directory directly.
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            if is_symlink && file_type.is_symlink_dir() {
                is_dir = true;
            }
        }

        // `DirEntry::file_type` and `DirEntry::metadata` use `lstat` semantics, so
        // on Unix a symlink is always reported as a non-directory. Resolving the
        // link here keeps linked directories enterable (`is_dir` gates navigation)
        // and lets them render as directories instead of as `lrwxrwxrwx`, which
        // would otherwise look executable. `metadata` follows the link; a broken
        // link simply keeps `is_dir == false` and stays listed as a file.
        if is_symlink && !is_dir {
            is_dir = std::fs::metadata(entry.path()).is_ok_and(|m| m.is_dir());
        }

        let size = if is_dir { None } else { Some(meta.len()) };
        let modified = meta.modified().ok();
        let attributes = get_attributes(&meta, is_dir, is_symlink);

        Ok(FileEntry {
            name,
            is_dir,
            is_symlink,
            size,
            modified,
            attributes,
            selected: false,
        })
    }
}

#[must_use]
pub fn mode_to_attributes(mode: u32, is_dir: bool, is_symlink: bool) -> String {
    let type_char = if is_symlink {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    };
    let bits = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    let mut s = String::with_capacity(10);
    s.push(type_char);
    for (mask, ch) in bits {
        s.push(if mode & mask != 0 { ch } else { '-' });
    }
    s
}

#[must_use]
pub fn get_attributes(meta: &Metadata, is_dir: bool, is_symlink: bool) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        let s = mode_to_attributes(mode, is_dir, is_symlink);
        format!("{s:<10}") // pad/truncate to 10
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        const FILE_ATTRIBUTE_NOT_CONTENT_INDEXED: u32 = 0x2000;
        const FILE_ATTRIBUTE_PINNED: u32 = 0x80000;
        let flags = meta.file_attributes();
        let type_char = if is_symlink {
            'l'
        } else if is_dir {
            'd'
        } else {
            '-'
        };
        let s = format!(
            "{type_char}{}{}{}{}{}",
            if flags & FILE_ATTRIBUTE_READONLY != 0 {
                'r'
            } else {
                '-'
            },
            if flags & FILE_ATTRIBUTE_HIDDEN != 0 {
                'h'
            } else {
                '-'
            },
            if flags & FILE_ATTRIBUTE_SYSTEM != 0 {
                's'
            } else {
                '-'
            },
            if flags & FILE_ATTRIBUTE_NOT_CONTENT_INDEXED != 0 {
                'i'
            } else {
                '-'
            },
            if flags & FILE_ATTRIBUTE_PINNED != 0 {
                'p'
            } else {
                '-'
            },
        );
        format!("{s:>6}") // pad/truncate to 6 (right-aligned)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = meta;
        let s = if is_symlink {
            "<LNK>"
        } else if is_dir {
            "<DIR>"
        } else {
            "<FILE>"
        };
        format!("{s:<10}")
    }
}

/// Resolves directory links (symlinks/junctions) to their real target so the
/// directory can be enumerated. On Windows, listing through the link path can
/// fail with `ERROR_ACCESS_DENIED` (os error 5). No-op on other platforms.
#[must_use]
fn resolve_dir_link(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            // `canonicalize` follows the link to its real target and returns a
            // clean DOS path (no `\\?\` prefix).
            if let Ok(resolved) = std::fs::canonicalize(path) {
                return resolved;
            }
        }
    }
    path.to_path_buf()
}

/// Lists directory contents including ".." for parent navigation.
///
/// # Errors
///
/// Returns an error if the directory cannot be read.
pub fn list_dir(path: &Path) -> Result<Vec<FileEntry>> {
    let path = resolve_dir_link(path);
    let mut entries = vec![];
    // Always add .. for going up
    entries.push(FileEntry {
        name: "..".to_string(),
        is_dir: true,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    });
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        if let Ok(file_entry) = FileEntry::try_from_dir_entry(&entry) {
            entries.push(file_entry);
        }
    }
    // Sort: dirs first, then files, both alphabetically
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(entries)
}

/// Creates a directory at the given path.
///
/// # Errors
///
/// Returns an error if the directory already exists or cannot be created.
pub fn create_directory(path: &std::path::Path) -> anyhow::Result<()> {
    if path.exists() {
        return Err(anyhow::anyhow!("Directory already exists"));
    }
    std::fs::create_dir_all(path)?;
    Ok(())
}

const G: f64 = 1_073_741_824.0;
const M: f64 = 1_048_576.0;
const K: f64 = 1_024.0;

#[must_use]
pub fn format_size(size: Option<u64>, is_dir: bool, is_symlink: bool) -> String {
    // Use up to 1 decimal precision, units G/M/K, no space, pad <DIR>/<LNK> to 7 chars
    if is_dir {
        if is_symlink {
            format!("{:>7}", "<LNK>")
        } else {
            format!("{:>7}", "<DIR>")
        }
    } else if let Some(s) = size {
        #[allow(clippy::cast_precision_loss)]
        if s >= 1_073_741_824 {
            format!("{:>6.1}G", s as f64 / G)
        } else if s >= 1_048_576 {
            format!("{:>6.1}M", s as f64 / M)
        } else if s >= 1_024 {
            format!("{:>6.1}K", s as f64 / K)
        } else {
            format!("{s:>7}")
        }
    } else {
        "       ".to_string()
    }
}

#[must_use]
pub fn format_modified(modified: Option<SystemTime>) -> String {
    if let Some(m) = modified {
        let dt: DateTime<Local> = m.into();
        let s = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        format!("{:<19}", s.chars().take(19).collect::<String>()) // pad/truncate to 19
    } else {
        "                   ".to_string() // 19 spaces
    }
}

/// Returns true if the file is a Windows GUI executable.
/// Returns false for console apps, scripts, or errors.
#[cfg(any(windows, test))]
#[must_use]
pub fn is_gui_executable(path: &std::path::Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };

    let mut buffer = [0u8; 64];
    if file.read_exact(&mut buffer).is_err() {
        return false;
    }

    // MZ header
    if &buffer[0..2] != b"MZ" {
        return false;
    }

    // Offset to PE header at 0x3C
    let pe_offset = u64::from(u32::from_le_bytes([
        buffer[60], buffer[61], buffer[62], buffer[63],
    ]));

    if file.seek(SeekFrom::Start(pe_offset)).is_err() {
        return false;
    }

    let mut pe_header = [0u8; 96]; // Signature (4) + File Header (20) + enough of Optional Header (72)
    if file.read_exact(&mut pe_header).is_err() {
        return false;
    }

    // PE signature
    if &pe_header[0..4] != b"PE\0\0" {
        return false;
    }

    // Subsystem is at offset 68 in Optional Header.
    // Optional Header starts after File Header (20 bytes) and Signature (4 bytes).
    // So Subsystem is at index 4 + 20 + 68 = 92.
    let subsystem = u16::from_le_bytes([pe_header[92], pe_header[93]]);

    subsystem == 2 // IMAGE_SUBSYSTEM_WINDOWS_GUI
}

/// Interprets a Unix-style attributes string (`-rwxr-xr-x`), returning whether
/// any class (user, group, other) has execute permission.
///
/// Returns `None` when `attributes` is not in the 10-character Unix format —
/// the 6-character Windows attribute flags or an empty string — so callers can
/// fall back to platform-specific detection. The two formats are unambiguous:
/// the Windows flags are `r`, `h`, `s`, `i` and `p`, never `x`.
#[must_use]
pub fn unix_attributes_allow_execute(attributes: &str) -> Option<bool> {
    let chars: Vec<char> = attributes.chars().collect();
    if chars.len() != 10 {
        return None;
    }
    // Execute bits live at positions 3 (user), 6 (group) and 9 (other).
    Some(chars[3] == 'x' || chars[6] == 'x' || chars[9] == 'x')
}

// Helper to detect executables
#[must_use]
pub fn is_executable(full_path: &std::path::Path, e: &FileEntry) -> bool {
    // Directories are never considered executable for icon purposes
    if e.is_dir {
        return false;
    }

    if e.is_symlink {
        return false;
    }

    // Entries carrying Unix-style permissions are authoritative, on every
    // platform: remote filesystems and archives mark scripts and binaries
    // executable through an `x` bit, which has no relation to the file name.
    // This must hold on Windows too, where the name would otherwise decide.
    if let Some(allowed) = unix_attributes_allow_execute(&e.attributes) {
        return allowed;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // No Unix permissions available: ask the filesystem (works for local
        // files whose attributes were not populated).
        if let Ok(meta) = std::fs::symlink_metadata(full_path) {
            let mode = meta.permissions().mode();
            return mode & 0o111 != 0;
        }

        false
    }
    #[cfg(windows)]
    {
        // Local Windows files carry DOS attributes rather than Unix
        // permissions, so fall back to the file extension.
        if let Some(ext) = full_path.extension().and_then(|e| e.to_str()) {
            let ext = ext.to_ascii_lowercase();
            return matches!(ext.as_str(), "exe" | "bat" | "cmd" | "com");
        }
        false
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = full_path;
        false
    }
}

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

/// Returns true if the name is "." or "..".
#[must_use]
pub fn is_dot_or_dotdot(name: &str) -> bool {
    name == "." || name == ".."
}

/// True when the entries of a filesystem follow the Unix conventions, i.e. they
/// carry Unix-style permissions (`drwxr-xr-x`) and mark hidden entries with a
/// leading dot.
#[must_use]
pub fn has_unix_attributes(local: bool) -> bool {
    cfg!(not(windows)) || !local
}

/// Returns true if a name marks a hidden entry on a Unix-like filesystem, i.e.
/// it starts with a dot. `unix_attributes` is [`has_unix_attributes`] for the
/// filesystem the entry was listed from.
///
/// `.` and `..` are navigation entries rather than hidden ones, so they are
/// never reported as hidden.
#[must_use]
pub fn has_hidden_name(name: &str, unix_attributes: bool) -> bool {
    unix_attributes && !is_dot_or_dotdot(name) && name.starts_with('.')
}

/// Returns true if an attribute string carries the Windows
/// `FILE_ATTRIBUTE_HIDDEN` flag.
#[must_use]
pub fn has_hidden_attribute(attributes: &str) -> bool {
    attributes.as_bytes().get(2) == Some(&b'h')
}

/// Returns true if an entry is hidden on the filesystem it was listed from.
///
/// Unix-like filesystems only have the leading-dot convention, whereas Windows
/// marks hidden entries with `FILE_ATTRIBUTE_HIDDEN` instead, so a dot-prefixed
/// name alone is not enough there. Windows therefore relies solely on the DOS
/// flag, while Unix relies solely on the leading dot.
#[must_use]
pub fn is_hidden(name: &str, attributes: &str, unix_attributes: bool) -> bool {
    has_hidden_attribute(attributes) || has_hidden_name(name, unix_attributes)
}

/// Returns an optimal chunk size for SFTP transfers based on the total file size.
#[must_use]
pub fn calculate_optimal_chunk_size(file_size: u64) -> usize {
    match file_size {
        0..=512_000 => 512 * 1024,              // 512KB for tiny files (≤512KB)
        512_001..=8_000_000 => 2 * 1024 * 1024, // 2MB for small files (512KB-8MB)
        8_000_001..=200_000_000 => 8 * 1024 * 1024, // 8MB for medium files (8MB-200MB)
        _ => 16 * 1024 * 1024,                  // 16MB for large files (>200MB)
    }
}

/// Formats a Unix permission mode into a standard 10-character string (e.g., "-rwxr-xr-x").
#[must_use]
pub fn format_sftp_permissions(perm: u32) -> String {
    let mut s = String::with_capacity(10);
    // Type (simplified mask check)
    s.push(if (perm & 0o170_000) == 0o040_000 {
        'd'
    } else if (perm & 0o170_000) == 0o120_000 {
        'l'
    } else {
        '-'
    });
    // User
    s.push(if perm & 0o400 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o200 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o100 != 0 { 'x' } else { '-' });
    // Group
    s.push(if perm & 0o040 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o020 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o010 != 0 { 'x' } else { '-' });
    // Others
    s.push(if perm & 0o004 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o002 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o001 != 0 { 'x' } else { '-' });
    s
}

/// Builds a `du -sb` command string for remote execution, properly escaped.
#[must_use]
pub fn build_du_command(path: &str) -> String {
    // Manual single-quote escaping: wrap in ' and replace ' with '\''
    let escaped = path.replace('\'', "'\\''");
    format!("du -sb '{escaped}' 2>/dev/null || echo 0")
}

#[cfg(test)]
mod tests;
