use super::ArchiveFormat;
use crate::fs::archive_fs::ArchiveEntry;
use crate::fs::provider::TaskProgressContext;
use crate::fs::utils::{FileEntry, get_attributes};
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const IO_BUFFER_SIZE: usize = 64 * 1024;

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

    /// Opens the gzip file and returns a streaming decoder.
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened.
    fn open_decompressor(&self) -> Result<GzDecoder<BufReader<File>>> {
        let file = File::open(&self.path).context("Failed to open gzip file")?;
        Ok(GzDecoder::new(BufReader::new(file)))
    }

    /// Extracts the original modification time from the gzip header, if set.
    fn header_mtime(decoder: &GzDecoder<BufReader<File>>) -> Option<SystemTime> {
        decoder.header().and_then(|h| {
            let m = h.mtime();
            if m == 0 {
                None
            } else {
                Some(SystemTime::UNIX_EPOCH + Duration::from_secs(u64::from(m)))
            }
        })
    }

    /// Streams `reader` into `file` in fixed-size chunks, reporting each chunk
    /// as processed bytes. Never holds the whole payload in memory.
    ///
    /// # Errors
    ///
    /// Returns an error if the decompressed output exceeds `limit` bytes
    /// (zip-bomb guard) or an I/O error occurs.
    fn write_streamed<R: Read>(
        reader: &mut R,
        file: &mut File,
        progress: &TaskProgressContext,
        limit: u64,
    ) -> std::io::Result<()> {
        let mut buf = vec![0u8; IO_BUFFER_SIZE];
        let mut total = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n > 0 {
                total += n as u64;
                if total > limit {
                    return Err(std::io::Error::other(format!(
                        "decompressed archive exceeds the {limit} byte limit"
                    )));
                }
                file.write_all(&buf[..n])?;
                progress
                    .processed_bytes
                    .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
            }
            if n == 0 {
                return file.flush();
            }
        }
    }
}

impl ArchiveFormat for GzipHandler {
    fn scan(&self) -> Result<super::ScanResult> {
        let mut decoder = self.open_decompressor()?;
        let mtime = Self::header_mtime(&decoder);

        // Stream the payload through a fixed-size buffer, keeping only a byte
        // count, so large `.gz` files are never materialized in memory just
        // to report their size.
        let mut decompressed_size = 0u64;
        let mut buf = vec![0u8; IO_BUFFER_SIZE];
        loop {
            let n = decoder.read(&mut buf)?;
            if n == 0 {
                break;
            }
            decompressed_size += n as u64;
        }

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
            // The trait requires the full payload, so this call materializes
            // the file; `scan` and `extract` stream instead.
            let mut decoder = self.open_decompressor()?;
            let mut data = Vec::new();
            decoder
                .read_to_end(&mut data)
                .context("Failed to decompress gzip file")?;
            Ok(data)
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

        let mut decoder = self.open_decompressor()?;
        let mut file = File::create(&target)
            .with_context(|| format!("Failed to create {}", target.display()))?;

        if let Err(e) = Self::write_streamed(
            &mut decoder,
            &mut file,
            progress,
            super::common::MAX_TOTAL_EXTRACT_BYTES,
        ) {
            // Don't leave a partial file behind on failure.
            let _ = std::fs::remove_file(&target);
            return Err(e.into());
        }

        progress
            .processed_items
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }
}
