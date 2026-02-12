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

#[must_use]
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
        if s >= 1_073_741_824 {
            format!("{:>6.1}G", s as f64 / 1_073_741_824.0)
        } else if s >= 1_048_576 {
            format!("{:>6.1}M", s as f64 / 1_048_576.0)
        } else if s >= 1_024 {
            format!("{:>6.1}K", s as f64 / 1_024.0)
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

// Helper to detect executables
#[must_use]
pub fn is_executable(_full_path: &std::path::Path, e: &FileEntry) -> bool {
    // Directories are never considered executable for icon purposes
    if e.is_dir {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // First try to get metadata from the filesystem (works for local files)
        if let Ok(meta) = std::fs::symlink_metadata(_full_path) {
            let mode = meta.permissions().mode();
            return mode & 0o111 != 0;
        }

        // Fallback: parse the attributes field (works for remote files)
        // Attributes format: "-rwxr-xr-x" or "drwxr-xr-x"
        // Check if any of the execute bits (positions 3, 6, 9) are 'x'
        if e.attributes.len() >= 10 {
            let chars: Vec<char> = e.attributes.chars().collect();
            // Check user execute (position 3), group execute (position 6), other execute (position 9)
            return chars.get(3) == Some(&'x')
                || chars.get(6) == Some(&'x')
                || chars.get(9) == Some(&'x');
        }

        false
    }
    #[cfg(windows)]
    {
        let lower = e.name.to_lowercase();
        lower.ends_with(".exe") || lower.ends_with(".bat") || lower.ends_with(".cmd")
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
        assert_eq!(format_size(Some(1024), false, false), "   1.0K");
        assert_eq!(format_size(Some(1536), false, false), "   1.5K");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(Some(1_048_576), false, false), "   1.0M");
        assert_eq!(format_size(Some(1_572_864), false, false), "   1.5M");
    }

    #[test]
    fn test_format_size_gigabytes() {
        assert_eq!(format_size(Some(1_073_741_824), false, false), "   1.0G");
        assert_eq!(format_size(Some(2_684_354_560), false, false), "   2.5G");
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
        use crate::fs::fs_local::LocalFs;
        use crate::fs::fs_provider::FileSystemProvider;
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
    fn test_is_executable_from_attributes() {
        #[cfg(unix)]
        {
            // Test executable detection from attributes field (for remote files)
            let executable_entry = FileEntry {
                name: "remote_script".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(1024),
                modified: None,
                attributes: "-rwxr-xr-x".to_string(),
                selected: false,
            };

            // Use a non-existent path to force fallback to attributes parsing
            let fake_path = std::path::PathBuf::from("/nonexistent/remote_script");
            assert!(is_executable(&fake_path, &executable_entry));

            // Test non-executable file
            let non_executable_entry = FileEntry {
                name: "remote_file".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(1024),
                modified: None,
                attributes: "-rw-r--r--".to_string(),
                selected: false,
            };

            assert!(!is_executable(&fake_path, &non_executable_entry));

            // Test directory (should not be executable even with x bits)
            let dir_entry = FileEntry {
                name: "remote_dir".to_string(),
                is_dir: true,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: "drwxr-xr-x".to_string(),
                selected: false,
            };

            assert!(!is_executable(&fake_path, &dir_entry));

            // Test file with only user execute permission
            let user_exec_entry = FileEntry {
                name: "user_exec".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(512),
                modified: None,
                attributes: "-rwx------".to_string(),
                selected: false,
            };

            assert!(is_executable(&fake_path, &user_exec_entry));
        }

        #[cfg(windows)]
        {
            // Test executable detection by file extension on Windows
            let fake_path = std::path::PathBuf::from("C:\\nonexistent\\file.exe");

            let exe_entry = FileEntry {
                name: "program.exe".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(1024),
                modified: None,
                attributes: "<FILE>".to_string(),
                selected: false,
            };

            assert!(is_executable(&fake_path, &exe_entry));

            let bat_entry = FileEntry {
                name: "script.bat".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(512),
                modified: None,
                attributes: "<FILE>".to_string(),
                selected: false,
            };

            assert!(is_executable(&fake_path, &bat_entry));

            let cmd_entry = FileEntry {
                name: "command.cmd".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(256),
                modified: None,
                attributes: "<FILE>".to_string(),
                selected: false,
            };

            assert!(is_executable(&fake_path, &cmd_entry));

            let txt_entry = FileEntry {
                name: "document.txt".to_string(),
                is_dir: false,
                is_symlink: false,
                size: Some(1024),
                modified: None,
                attributes: "<FILE>".to_string(),
                selected: false,
            };

            assert!(!is_executable(&fake_path, &txt_entry));

            let dir_entry = FileEntry {
                name: "folder".to_string(),
                is_dir: true,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: "<DIR>".to_string(),
                selected: false,
            };

            assert!(!is_executable(&fake_path, &dir_entry));
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
