use anyhow::Result;
use chrono::{DateTime, Local};
use std::fs::{self, Metadata};
use std::path::PathBuf;
use std::time::SystemTime;

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
    pub fn from_path(path: &PathBuf, meta: &Metadata) -> Self {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let is_dir = meta.is_dir();
        let is_symlink = match fs::symlink_metadata(path) {
            Ok(m) => m.file_type().is_symlink(),
            Err(_) => false,
        };
        let size = if is_dir { None } else { Some(meta.len()) };
        let modified = meta.modified().ok();
        let attributes = get_attributes(meta, is_dir);
        FileEntry {
            name,
            is_dir,
            is_symlink,
            size,
            modified,
            attributes,
            selected: false,
        }
    }
}

pub fn get_attributes(meta: &Metadata, is_dir: bool) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
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
        attrs
    }
    #[cfg(not(unix))]
    {
        if is_dir {
            "<DIR>".to_string()
        } else {
            "<FILE>".to_string()
        }
    }
}

pub fn list_dir(path: &PathBuf) -> Result<Vec<FileEntry>> {
    let mut entries = vec![];
    // Always add .. for going up
    entries.push(FileEntry {
        name: "..".to_string(),
        is_dir: true,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: "".to_string(),
        selected: false,
    });
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_path = entry.path();
        let meta = match fs::metadata(&file_path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        entries.push(FileEntry::from_path(&file_path, &meta));
    }
    // Sort: dirs first, then files, both alphabetically
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(entries)
}

pub fn read_file_content(path: &std::path::Path, limit: usize) -> anyhow::Result<String> {
    use std::fs::File;
    use std::io::Read;

    let metadata = std::fs::metadata(path)?;
    if metadata.len() > limit as u64 {
        return Ok(format!(
            "File too large to display (size: {}, limit: {})",
            format_size(Some(metadata.len()), false, false),
            format_size(Some(limit as u64), false, false)
        ));
    }

    let file = File::open(path)?;
    let mut buffer = Vec::new();
    // Read up to limit + 1 to detect if it's exactly limit or more (though metadata check covers most cases)
    file.take((limit + 1) as u64).read_to_end(&mut buffer)?;

    // Check for binary content (null bytes in first 8KB)
    let check_len = buffer.len().min(8192);
    if buffer[..check_len].contains(&0) {
        return Ok("Binary file detected".to_string());
    }

    // Try to convert to string
    match String::from_utf8(buffer) {
        Ok(s) => Ok(s),
        Err(_) => Ok("File content is not valid UTF-8".to_string()),
    }
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
            format!("{:>7}", s)
        }
    } else {
        "       ".to_string()
    }
}

pub fn format_modified(modified: Option<SystemTime>) -> String {
    if let Some(m) = modified {
        let dt: DateTime<Local> = m.into();
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        "".to_string()
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
        assert_eq!(format_modified(None), "");
    }

    #[test]
    fn test_read_file_content_valid() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("fm_test_read.txt");
        let mut file = std::fs::File::create(&test_file).unwrap();
        file.write_all(b"Hello, World!").unwrap();

        let result = read_file_content(&test_file, 1024);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, World!");

        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_read_file_content_too_large() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("fm_test_large.txt");
        let mut file = std::fs::File::create(&test_file).unwrap();
        file.write_all(&vec![b'a'; 100]).unwrap();

        let result = read_file_content(&test_file, 50);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("too large"));

        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_read_file_content_binary() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("fm_test_binary.bin");
        let mut file = std::fs::File::create(&test_file).unwrap();
        file.write_all(&[0u8, 1, 2, 0, 3]).unwrap();

        let result = read_file_content(&test_file, 1024);
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
}
