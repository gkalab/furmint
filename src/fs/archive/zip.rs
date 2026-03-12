use super::ArchiveFormat;
use super::common::{self, ArchiveEntryMetadata};
use crate::fs::traits::TaskProgressContext;
use crate::fs::utils::mode_to_attributes;
use anyhow::{Context, Result};
use chrono::TimeZone;
use filetime::{FileTime, set_file_mtime};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::SystemTime;

pub struct ZipHandler {
    path: PathBuf,
}

impl ZipHandler {
    /// Creates a new `ZipHandler`.
    ///
    /// # Errors
    ///
    /// Currently always returns Ok, but may return errors in the future.
    ///
    /// # Panics
    ///
    /// This function never panics.
    #[must_use]
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn zip_dt_to_system_time(dt: Option<zip::DateTime>) -> SystemTime {
        if let Some(dt) = dt {
            chrono::Utc
                .with_ymd_and_hms(
                    i32::from(dt.year()),
                    u32::from(dt.month()),
                    u32::from(dt.day()),
                    u32::from(dt.hour()),
                    u32::from(dt.minute()),
                    u32::from(dt.second()),
                )
                .single()
                .map_or(SystemTime::UNIX_EPOCH, SystemTime::from)
        } else {
            SystemTime::UNIX_EPOCH
        }
    }
}

impl ZipHandler {
    fn do_scan(&self) -> Result<super::ScanResult> {
        let file = File::open(&self.path).context("Failed to open zip archive")?;
        let reader = std::io::BufReader::new(file);
        let mut archive = zip::ZipArchive::new(reader).context("Failed to read zip archive")?;

        let mut entries_map = HashMap::new();
        let mut tree_map = HashMap::new();

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let path = common::normalize_path(file.name());

            let is_dir = file.is_dir() || file.name().ends_with('/');
            let size = file.size();
            let modified = Self::zip_dt_to_system_time(file.last_modified());

            let unix_mode = file.unix_mode();
            let attributes = if let Some(mode) = unix_mode {
                mode_to_attributes(mode, is_dir, false)
            } else if is_dir {
                "dr-xr-xr-x".to_string()
            } else {
                "-r--r--r--".to_string()
            };

            common::add_to_tree(
                path,
                ArchiveEntryMetadata {
                    is_dir,
                    is_symlink: false,
                    size: Some(size),
                    modified: Some(modified),
                    attributes,
                    position: None,
                },
                &mut entries_map,
                &mut tree_map,
            );
        }

        Ok((entries_map, common::finalize_tree(tree_map)))
    }

    fn read_file(&self, path_str: &str) -> Result<Vec<u8>> {
        let file = File::open(&self.path).context("Failed to open archive")?;
        let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;
        let mut zip_file = archive.by_name(path_str).context("File not found in zip")?;
        let size = usize::try_from(zip_file.size()).context("Zip file entry too large")?;
        let mut buffer = Vec::with_capacity(size);
        zip_file.read_to_end(&mut buffer)?;
        Ok(buffer)
    }

    fn extract(
        &self,
        src_str: &str,
        dest: &Path,
        _is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        let file = File::open(&self.path).context("Failed to open archive")?;
        let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;

        let is_root = src_str.is_empty() || src_str == ".";
        let mut dir_mtimes = Vec::new();
        let mut last_update = std::time::Instant::now();

        // Optimized single file extraction
        if !is_root
            && let Ok(mut zip_file) = archive.by_name(src_str)
            && !zip_file.is_dir()
        {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = File::create(dest)?;
            std::io::copy(&mut zip_file, &mut out)?;
            let size = zip_file.size();
            progress.processed_bytes.fetch_add(size, Ordering::Relaxed);
            let p = progress.processed_items.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = progress
                .tx
                .send(crate::tasks::TaskEvent::UpdateProgress(progress.id, p, 0));
            return Ok(());
        }

        for i in 0..archive.len() {
            if progress.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let mut zip_file = archive.by_index(i).context("Failed to get zip index")?;
            let name_raw = zip_file.name().to_string();
            let name = name_raw.replace('\\', "/");
            let name = name.trim_end_matches('/');

            let should_extract = if is_root {
                true
            } else {
                name == src_str || name.starts_with(&format!("{src_str}/"))
            };

            if should_extract {
                let rel_path = if is_root {
                    PathBuf::from(name)
                } else {
                    Path::new(name)
                        .strip_prefix(src_str)
                        .map_or_else(|_| PathBuf::from(name), std::path::Path::to_path_buf)
                };

                let rel_name_str = rel_path.to_string_lossy().to_string();
                if rel_name_str.is_empty() && zip_file.is_dir() {
                    continue;
                }

                let target = if rel_name_str.is_empty() {
                    dest.to_path_buf()
                } else {
                    dest.join(&rel_name_str)
                };
                let mtime = Self::zip_dt_to_system_time(zip_file.last_modified());

                if zip_file.is_dir() || name_raw.ends_with('/') {
                    std::fs::create_dir_all(&target)?;
                    dir_mtimes.push((target, mtime));
                } else {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut out = File::create(&target)?;
                    std::io::copy(&mut zip_file, &mut out)?;
                    let size = zip_file.size();
                    progress.processed_bytes.fetch_add(size, Ordering::Relaxed);
                    let _ = set_file_mtime(&target, FileTime::from_system_time(mtime));
                }

                let p = progress.processed_items.fetch_add(1, Ordering::Relaxed) + 1;
                let now = std::time::Instant::now();
                if p.is_multiple_of(10)
                    || now.duration_since(last_update) > std::time::Duration::from_millis(100)
                {
                    let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateCurrentFile(
                        progress.id,
                        rel_name_str,
                    ));
                    let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                        progress.id,
                        p,
                        0,
                    ));
                    last_update = now;
                }
            }
        }

        common::preserve_mtimes(dir_mtimes);

        let p = progress.processed_items.load(Ordering::Relaxed);
        let _ = progress
            .tx
            .send(crate::tasks::TaskEvent::UpdateProgress(progress.id, p, 0));

        Ok(())
    }
}

impl ArchiveFormat for ZipHandler {
    /// Scans the archive and returns its contents.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be read.
    fn scan(&self) -> Result<super::ScanResult> {
        self.do_scan()
    }

    /// Reads a file from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_file(&self, path_str: &str) -> Result<Vec<u8>> {
        self.read_file(path_str)
    }

    /// Extracts a file or directory from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if extraction fails.
    fn extract(
        &self,
        src_str: &str,
        dest: &Path,
        is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        self.extract(src_str, dest, is_dir, progress)
    }
}
