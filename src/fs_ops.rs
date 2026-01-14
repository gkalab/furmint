use anyhow::Result;
use chrono::{DateTime, Local};
use std::fs::{self, Metadata};
use std::path::Path;
use std::time::SystemTime;

// Cross-platform: empties user trash. Returns number of deleted items, or error.
#[allow(clippy::unused_async)] // async kept for API consistency even if not currently awaiting
pub async fn empty_trash() -> std::result::Result<usize, String> {
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

            if SUCCEEDED(res) {
                Ok(0)
            } else {
                Err(format!("Failed: SHEmptyRecycleBinW error code {:#x}", res))
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
        for entry in fs::read_dir(&trash_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
            } else {
                fs::remove_file(&path).map_err(|e| e.to_string())?;
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
                for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let path = entry.path();
                    if path.is_dir() {
                        fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
                    } else {
                        fs::remove_file(&path).map_err(|e| e.to_string())?;
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

#[derive(Clone)]
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
    pub fn try_from_dir_entry(entry: &std::fs::DirEntry) -> Result<Self> {
        let file_type = entry.file_type()?;
        let meta = entry.metadata()?;

        let name = entry.file_name().to_string_lossy().to_string();
        let is_symlink = file_type.is_symlink();
        let is_dir = file_type.is_dir();

        // On Windows/Unix, we might want to follow symlinks to see if they are directories
        // However, following symlinks is expensive over network drives.
        // For now, we trust the entry's reported is_dir.
        // If it's a symlink, is_dir will be true only if it's a directory symlink/junction
        // that was resolved by the OS during iteration (common on Windows).

        let size = if is_dir { None } else { Some(meta.len()) };
        let modified = meta.modified().ok();
        let attributes = get_attributes(&meta, is_dir);

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

pub fn get_attributes(_meta: &Metadata, is_dir: bool) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = _meta.permissions().mode();
        let mut attrs = String::new();
        attrs.push(if is_dir { 'd' } else { '-' });
        for i in (0..9).rev() {
            let bit = (mode >> i) & 1;
            attrs.push(match i % 3 {
                2 => {
                    if bit == 1 {
                        'r'
                    } else {
                        '-'
                    }
                }
                1 => {
                    if bit == 1 {
                        'w'
                    } else {
                        '-'
                    }
                }
                0 => {
                    if bit == 1 {
                        'x'
                    } else {
                        '-'
                    }
                }
                _ => '-',
            });
        }
        let s: String = attrs.chars().take(10).collect();
        format!("{s:<10}") // pad/truncate to 10
    }
    #[cfg(not(unix))]
    {
        let s = if is_dir { "<DIR>" } else { "<FILE>" };
        format!("{:<10}", s)
    }
}

pub fn list_dir(path: &Path) -> Result<Vec<FileEntry>> {
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
    for entry in fs::read_dir(path)? {
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

pub fn create_directory(path: &std::path::Path) -> anyhow::Result<()> {
    if path.exists() {
        return Err(anyhow::anyhow!("Directory already exists"));
    }
    std::fs::create_dir_all(path)?;
    Ok(())
}

pub fn format_size(size: Option<u64>, is_dir: bool, is_symlink: bool) -> String {
    // Use up to 1 decimal precision, units G/M/K, no space, pad <DIR>/<LNK> to 7 chars
    if is_dir {
        if is_symlink {
            format!("{:>7}", "<LNK>")
        } else {
            format!("{:>7}", "<DIR>")
        }
    } else if let Some(s) = size {
        if s >= 1_000_000_000 {
            format!("{:>6.1}G", s as f64 / 1_000_000_000.0)
        } else if s >= 1_000_000 {
            format!("{:>6.1}M", s as f64 / 1_000_000.0)
        } else if s >= 1_000 {
            format!("{:>6.1}K", s as f64 / 1_000.0)
        } else {
            format!("{s:>7}")
        }
    } else {
        "       ".to_string()
    }
}

pub fn format_modified(modified: Option<SystemTime>) -> String {
    if let Some(m) = modified {
        let dt: DateTime<Local> = m.into();
        let s = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        format!("{:<19}", s.chars().take(19).collect::<String>()) // pad/truncate to 19
    } else {
        "                   ".to_string() // 19 spaces
    }
}

// Helper to detect executables
pub fn is_executable(_full_path: &std::path::Path, e: &FileEntry) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::symlink_metadata(_full_path) {
            let mode = meta.permissions().mode();
            mode & 0o111 != 0 && !e.is_dir
        } else {
            false
        }
    }
    #[cfg(windows)]
    {
        let lower = e.name.to_lowercase();
        (lower.ends_with(".exe") || lower.ends_with(".bat") || lower.ends_with(".cmd")) && !e.is_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(Some(500), false, false), "    500");
        assert_eq!(format_size(Some(0), false, false), "      0");
        assert_eq!(format_size(Some(999), false, false), "    999");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(Some(1000), false, false), "   1.0K");
        assert_eq!(format_size(Some(1500), false, false), "   1.5K");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(Some(1_000_000), false, false), "   1.0M");
        assert_eq!(format_size(Some(1_500_000), false, false), "   1.5M");
    }

    #[test]
    fn test_format_size_gigabytes() {
        assert_eq!(format_size(Some(1_000_000_000), false, false), "   1.0G");
        assert_eq!(format_size(Some(2_500_000_000), false, false), "   2.5G");
    }

    #[test]
    fn test_format_size_dir() {
        assert_eq!(format_size(None, true, false), "  <DIR>");
        assert_eq!(format_size(Some(100), true, false), "  <DIR>");
    }

    #[test]
    fn test_format_size_symlink() {
        assert_eq!(format_size(None, true, true), "  <LNK>");
    }

    #[test]
    fn test_format_modified_some() {
        let now = SystemTime::now();
        let result = format_modified(Some(now));
        assert_eq!(result.len(), 19);
        assert!(result.contains('-'));
        assert!(result.contains(':'));
    }

    #[test]
    fn test_format_modified_none() {
        assert_eq!(format_modified(None), "                   "); // 19 spaces for UI alignment
    }

    #[test]
    fn test_read_file_content() {
        use crate::fs_local::LocalFs;
        use crate::fs_provider::FileSystemProvider;
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("fm_test_read.txt");
        let mut file = std::fs::File::create(&test_file).unwrap();
        file.write_all(b"Hello, World!").unwrap();

        let provider = LocalFs::new();
        let result = provider.read_file_content(&test_file, 1024);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, World!");

        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_read_file_content_too_large() {
        use crate::fs_local::LocalFs;
        use crate::fs_provider::FileSystemProvider;
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("fm_test_large.txt");
        let mut file = std::fs::File::create(&test_file).unwrap();
        file.write_all(&[b'a'; 100]).unwrap();

        let provider = LocalFs::new();
        let result = provider.read_file_content(&test_file, 50);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("too large"));

        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_read_file_content_binary() {
        use crate::fs_local::LocalFs;
        use crate::fs_provider::FileSystemProvider;
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("fm_test_binary.bin");
        let mut file = std::fs::File::create(&test_file).unwrap();
        file.write_all(&[0u8, 1, 2, 0, 3]).unwrap();

        let provider = LocalFs::new();
        let result = provider.read_file_content(&test_file, 1024);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Binary"));

        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_list_dir_includes_parent() {
        let temp_dir = std::env::temp_dir();
        let result = list_dir(&temp_dir);
        assert!(result.is_ok());
        let entries = result.unwrap();
        assert!(!entries.is_empty());
        assert_eq!(entries[0].name, "..");
        assert!(entries[0].is_dir);
    }

    #[test]
    fn test_get_attributes_unix() {
        #[cfg(unix)]
        {
            let temp_dir = std::env::temp_dir();
            let meta = std::fs::metadata(&temp_dir).unwrap();
            let attrs = get_attributes(&meta, true);
            assert_eq!(attrs.chars().next().unwrap(), 'd');
            assert_eq!(attrs.len(), 10);
        }
    }

    #[test]
    fn test_is_executable() {
        #[cfg(unix)]
        {
            use std::fs::{self, File};
            use std::os::unix::fs::PermissionsExt;
            let temp_dir = std::env::temp_dir();
            let file_path = temp_dir.join("fm_test_exe");
            {
                let _ = File::create(&file_path).unwrap();
            }
            let mut perms = fs::metadata(&file_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&file_path, perms).unwrap();

            let entry = FileEntry {
                name: "fm_test_exe".to_string(),
                is_dir: false,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: String::new(),
                selected: false,
            };

            assert!(is_executable(&file_path, &entry));

            perms = fs::metadata(&file_path).unwrap().permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&file_path, perms).unwrap();
            assert!(!is_executable(&file_path, &entry));

            fs::remove_file(&file_path).ok();
        }
    }

    #[test]
    fn test_create_directory() {
        let temp_dir = std::env::temp_dir();
        let new_dir = temp_dir.join("fm_test_create_dir");
        if new_dir.exists() {
            fs::remove_dir_all(&new_dir).ok();
        }

        let result = create_directory(&new_dir);
        assert!(result.is_ok());
        assert!(new_dir.exists());

        let result_err = create_directory(&new_dir);
        assert!(result_err.is_err());

        fs::remove_dir_all(&new_dir).ok();
    }
}
