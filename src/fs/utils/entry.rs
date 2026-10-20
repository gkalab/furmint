use super::hidden::is_hidden;
use anyhow::Result;
use chrono::{DateTime, Local};
use std::fs::Metadata;
use std::time::SystemTime;

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
    /// Whether the entry is hidden on the filesystem it was listed from.
    ///
    /// `unix_attributes` is [`super::hidden::has_unix_attributes`] for that filesystem (see
    /// [`crate::fs::provider::FileSystemProvider::unix_attributes`]): a Unix
    /// filesystem hides dotfiles, a local Windows one uses the attribute flag.
    #[must_use]
    pub fn is_hidden(&self, unix_attributes: bool) -> bool {
        is_hidden(&self.name, &self.attributes, unix_attributes)
    }

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
