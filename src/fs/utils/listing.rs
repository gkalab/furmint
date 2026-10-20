use super::entry::FileEntry;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

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
