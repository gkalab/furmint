//! Test suite for `fs::archive::common`.
mod extract;
mod metadata;
mod path;

use super::*;
use crate::fs::provider::TaskProgressContext;
use crate::tasks::UiEvent;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

fn test_progress() -> TaskProgressContext {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let tx = crate::tasks::EventBus::new(tx);
    TaskProgressContext {
        id: 1,
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        processed_bytes: Arc::new(AtomicU64::new(0)),
        total_bytes: 0,
    }
}

fn extract_blob(blob: &[u8], dest: &Path, src_str: &str) -> Result<bool> {
    let mut cursor = Cursor::new(blob);
    let mut archive = tar::Archive::new(&mut cursor);
    let opts_progress = test_progress();
    let dest = canonicalize_dest(dest);
    let opts = ExtractOptions {
        src_str,
        dest: &dest,
        progress: &opts_progress,
    };
    let mut pending_dirs = Vec::new();
    let mut last_update = std::time::Instant::now();
    let mut result = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name_raw = entry.path()?.to_string_lossy().to_string();
        let is_dir = entry.header().entry_type().is_dir();
        let is_symlink = entry.header().entry_type().is_symlink();
        let size = entry.size();
        let mtime = entry
            .header()
            .mtime()
            .ok()
            .map(|m| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(m));
        let mode = entry.header().mode().ok();
        result = Some(handle_extraction_entry(
            &mut entry,
            &ExtractionEntryMetadata {
                name_raw: &name_raw,
                is_dir,
                is_symlink,
                size,
                mtime,
                mode,
            },
            &opts,
            &mut pending_dirs,
            &mut last_update,
        )?);
    }
    restore_dir_metadata(pending_dirs);
    Ok(result.unwrap_or(false))
}

/// Builds a tar blob, giving every entry its own mode. Entries whose name
/// ends in `/` are written as directory headers with no data.
#[cfg(unix)]
fn build_tar_with_modes(entries: &[(&str, u32, &[u8])]) -> Vec<u8> {
    let mut b = Vec::new();
    {
        let mut w = tar::Builder::new(&mut b);
        for (name, mode, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_mode(*mode);
            header.set_path(name).unwrap();
            if name.ends_with('/') {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
            } else {
                header.set_size(data.len() as u64);
            }
            header.set_cksum();
            w.append(&header, *data).unwrap();
        }
        w.finish().unwrap();
    }
    b
}

fn build_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut b = Vec::new();
    {
        let mut w = tar::Builder::new(&mut b);
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            w.append_data(&mut header, name, *data).unwrap();
        }
        w.finish().unwrap();
    }
    b
}

/// Extracts a batch of hand-built entries the way a format handler does: one
/// `handle_extraction_entry` call per entry, then a single deferred
/// directory restoration once the batch is complete.
#[cfg(unix)]
fn extract_entries(dest: &Path, entries: &[(ExtractionEntryMetadata<'_>, &[u8])]) -> Result<()> {
    let opts_progress = test_progress();
    let dest = canonicalize_dest(dest);
    let opts = ExtractOptions {
        src_str: "",
        dest: &dest,
        progress: &opts_progress,
    };
    let mut pending_dirs = Vec::new();
    for (meta, data) in entries {
        handle_extraction_entry(
            &mut Cursor::new(*data),
            meta,
            &opts,
            &mut pending_dirs,
            &mut std::time::Instant::now(),
        )?;
    }
    restore_dir_metadata(pending_dirs);
    Ok(())
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[cfg(unix)]
fn make_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

fn extract_plain_name(name: &str, dest: &Path) -> Result<bool> {
    let opts_progress = test_progress();
    let dest = canonicalize_dest(dest);
    let opts = ExtractOptions {
        src_str: "",
        dest: &dest,
        progress: &opts_progress,
    };
    let mut cursor = Cursor::new(b"evil".as_slice());
    handle_extraction_entry(
        &mut cursor,
        &ExtractionEntryMetadata {
            name_raw: name,
            is_dir: false,
            is_symlink: false,
            size: 4,
            mtime: None,
            mode: None,
        },
        &opts,
        &mut Vec::new(),
        &mut std::time::Instant::now(),
    )
}

fn dir_meta(path: &str) -> PendingDirMeta {
    PendingDirMeta {
        path: PathBuf::from(path),
        mtime: Some(SystemTime::UNIX_EPOCH),
        mode: None,
    }
}

fn link_map(pairs: &[(&str, &str)]) -> HashMap<PathBuf, PathBuf> {
    pairs
        .iter()
        .map(|(link, target)| (PathBuf::from(link), PathBuf::from(target)))
        .collect()
}

fn entry(name: &str, is_dir: bool, is_symlink: bool, size: Option<u64>) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink,
        size,
        modified: None,
        attributes: String::new(),
        selected: false,
    }
}
