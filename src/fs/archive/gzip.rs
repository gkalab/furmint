use super::ArchiveFormat;
use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::fs_provider::TaskProgressContext;
use crate::fs::utils::{FileEntry, get_attributes};
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct GzipHandler {
    path: PathBuf,
    original_name: String,
}

impl GzipHandler {
    /// Creates a new `GzipHandler` for the given path.
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened.
    pub fn new(path: &Path) -> Result<Self> {
        let original_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map_or_else(|| "unknown".to_string(), std::string::ToString::to_string);

        Ok(Self {
            path: path.to_path_buf(),
            original_name,
        })
    }

    fn decompress(&self) -> Result<(Vec<u8>, Option<SystemTime>)> {
        let file = File::open(&self.path).context("Failed to open gzip file")?;
        let reader = BufReader::new(file);
        let mut decoder = GzDecoder::new(reader);

        let mtime = decoder.header().and_then(|h| {
            let m = h.mtime();
            if m == 0 {
                None
            } else {
                Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(m)))
            }
        });

        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok((decompressed, mtime))
    }
}

impl ArchiveFormat for GzipHandler {
    fn scan(&self) -> Result<super::ScanResult> {
        let (data, mtime) = self.decompress()?;
        let decompressed_size = data.len() as u64;

        let metadata = std::fs::metadata(&self.path).ok();
        let attributes = metadata.as_ref().map_or_else(
            || "-rw-r--r--".to_string(),
            |m| get_attributes(m, false, false),
        );

        let entry = ArchiveEntry {
            file_entry: FileEntry {
                name: self.original_name.clone(),
                is_dir: false,
                is_symlink: false,
                size: Some(decompressed_size),
                modified: mtime,
                attributes,
                selected: false,
            },
            position: Some(0),
        };

        let mut entries_map = std::collections::HashMap::new();
        entries_map.insert(PathBuf::from(&self.original_name), entry);

        let mut tree_map = std::collections::HashMap::new();
        tree_map.insert(PathBuf::from("."), vec![PathBuf::from(&self.original_name)]);

        Ok((entries_map, tree_map))
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let path_clean = path.trim_end_matches('/');
        if path_clean == self.original_name || path_clean.is_empty() || path_clean == "." {
            Ok(self.decompress()?.0)
        } else {
            Err(anyhow::anyhow!(
                "File not found in gzip: {} (only contains: {})",
                path_clean,
                self.original_name
            ))
        }
    }

    fn extract(
        &self,
        src_str: &str,
        dest: &Path,
        _is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        let path_clean = src_str.trim_end_matches('/');

        if !path_clean.is_empty() && path_clean != "." && path_clean != self.original_name {
            return Err(anyhow::anyhow!(
                "File not found in gzip: {} (only contains: {})",
                path_clean,
                self.original_name
            ));
        }

        let (data, _mtime) = self.decompress()?;

        let target = if path_clean.is_empty() || path_clean == "." {
            if dest.is_dir() {
                dest.join(&self.original_name)
            } else {
                dest.to_path_buf()
            }
        } else {
            dest.to_path_buf()
        };

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&target, &data)?;
        progress
            .processed_bytes
            .fetch_add(data.len() as u64, std::sync::atomic::Ordering::Relaxed);
        progress
            .processed_items
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }
}
