use std::fs::{self, Metadata};
use std::path::PathBuf;
use std::time::SystemTime;
use chrono::{DateTime, Local};
use anyhow::Result;

#[derive(Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub attributes: String,
}

impl FileEntry {
    pub fn from_path(path: &PathBuf, meta: &Metadata) -> Self {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let is_dir = meta.is_dir();
        let size = if is_dir { None } else { Some(meta.len()) };
        let modified = meta.modified().ok();
        let attributes = get_attributes(meta, is_dir);
        FileEntry { name, is_dir, size, modified, attributes }
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
                2 => if bit == 1 { 'r' } else { '-' },
                1 => if bit == 1 { 'w' } else { '-' },
                0 => if bit == 1 { 'x' } else { '-' },
                _ => '-',
            });
        }
        attrs
    }
    #[cfg(not(unix))]
    {
        if is_dir { "<DIR>".to_string() } else { "<FILE>".to_string() }
    }
}

pub fn list_dir(path: &PathBuf) -> Result<Vec<FileEntry>> {
    let mut entries = vec![];
    // Always add .. for going up
    entries.push(FileEntry {
        name: "..".to_string(),
        is_dir: true,
        size: None,
        modified: None,
        attributes: "".to_string(),
    });
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let file_path = entry.path();
        entries.push(FileEntry::from_path(&file_path, &meta));
    }
    // Sort: dirs first, then files, both alphabetically
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });
    Ok(entries)
}

pub fn format_size(size: Option<u64>, is_dir: bool) -> String {
    // Use up to 1 decimal precision, units G/M/K, no space, pad <DIR> to 7 chars
    if is_dir {
        format!("{:>7}", "<DIR>")
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
