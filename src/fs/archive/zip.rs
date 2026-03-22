use super::ArchiveFormat;
use super::common::{self, ArchiveEntryMetadata};
use crate::fs::traits::TaskProgressContext;
use crate::fs::utils::mode_to_attributes;
use anyhow::{Context, Result};
use chrono::TimeZone;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use zip::write::SimpleFileOptions;

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
        is_dir: bool,
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

        let opts = common::ExtractOptions {
            src_str,
            dest,
            is_dir,
            progress,
        };

        for i in 0..archive.len() {
            if progress.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let mut zip_file = archive.by_index(i).context("Failed to get zip index")?;
            let name_raw = zip_file.name().to_string();
            let is_entry_dir = zip_file.is_dir() | name_raw.ends_with('/');
            let size = zip_file.size();
            let mtime = Some(Self::zip_dt_to_system_time(zip_file.last_modified()));

            common::handle_extraction_entry(
                &mut zip_file,
                &common::ExtractionEntryMetadata {
                    name_raw: &name_raw,
                    is_dir: is_entry_dir,
                    is_symlink: false, // ZipFile doesn't expose symlink easily here, assuming false for now
                    size,
                    mtime,
                },
                &opts,
                &mut dir_mtimes,
                &mut last_update,
            )?;
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

    /// Deletes a file or directory from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn delete_file(&self, path_str: &str) -> Result<()> {
        let parent = self.path.parent().unwrap_or(Path::new("."));
        let mut temp_file = tempfile::NamedTempFile::new_in(parent)?;
        let temp_path = temp_file.path().to_path_buf();

        {
            let file = File::open(&self.path).context("Failed to open archive")?;
            let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;

            let mut writer = zip::ZipWriter::new(temp_file.as_file_mut());

            let path_norm = common::normalize_path(path_str);
            let path_norm_str = path_norm.to_string_lossy().to_string();

            for i in 0..archive.len() {
                let zip_file = archive.by_index(i).context("Failed to get zip index")?;
                let name = zip_file.name();
                let name_norm = common::normalize_path(name);
                let name_norm_str = name_norm.to_string_lossy().to_string();

                let should_delete = if name_norm_str == path_norm_str {
                    true
                } else {
                    name_norm_str.starts_with(&(path_norm_str.clone() + "/"))
                };

                if !should_delete {
                    writer.raw_copy_file(zip_file)?;
                }
            }
            writer.finish()?;
        }

        std::fs::rename(temp_path, &self.path)?;
        Ok(())
    }

    /// Adds a file to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten or the source file cannot be read.
    fn add_file(&self, src: &Path, dest_in_archive: &str) -> Result<()> {
        let parent = self.path.parent().unwrap_or(Path::new("."));
        let mut temp_file = tempfile::NamedTempFile::new_in(parent)?;
        let temp_path = temp_file.path().to_path_buf();

        let dest_norm = common::normalize_path(dest_in_archive);
        let dest_norm_str = dest_norm.to_string_lossy().to_string();

        {
            let mut writer = zip::ZipWriter::new(temp_file.as_file_mut());

            if self.path.exists() {
                let file = File::open(&self.path).context("Failed to open archive")?;
                let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;

                for i in 0..archive.len() {
                    let zip_file = archive.by_index(i).context("Failed to get zip index")?;
                    let name = zip_file.name();
                    let name_norm = common::normalize_path(name);
                    let name_norm_str = name_norm.to_string_lossy().to_string();

                    // If we're replacing an existing file, skip it
                    if name_norm_str != dest_norm_str {
                        writer.raw_copy_file(zip_file)?;
                    }
                }
            }

            // Add the new file
            let mut src_file = File::open(src).context("Failed to open source file")?;
            let metadata = src_file.metadata()?;
            let options = SimpleFileOptions::default()
                .last_modified_time(
                    metadata
                        .modified()
                        .ok()
                        .and_then(|t| {
                            use chrono::{Datelike, Timelike};
                            let dt: chrono::DateTime<chrono::Utc> = t.into();
                            zip::DateTime::from_date_and_time(
                                u16::try_from(dt.year()).unwrap_or(1980),
                                u8::try_from(dt.month()).unwrap_or(1),
                                u8::try_from(dt.day()).unwrap_or(1),
                                u8::try_from(dt.hour()).unwrap_or(0),
                                u8::try_from(dt.minute()).unwrap_or(0),
                                u8::try_from(dt.second()).unwrap_or(0),
                            )
                            .ok()
                        })
                        .unwrap_or_else(|| {
                            zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap()
                        }),
                )
                .unix_permissions(if cfg!(unix) {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode()
                } else {
                    0o644
                });

            writer.start_file(dest_in_archive, options)?;
            std::io::copy(&mut src_file, &mut writer)?;
            writer.finish()?;
        }

        std::fs::rename(temp_path, &self.path)?;
        Ok(())
    }

    /// Renames a file or directory within the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn rename_file(&self, from: &str, to: &str) -> Result<()> {
        let parent = self.path.parent().unwrap_or(Path::new("."));
        let mut temp_file = tempfile::NamedTempFile::new_in(parent)?;
        let temp_path = temp_file.path().to_path_buf();

        let from_norm = common::normalize_path(from);
        let from_norm_str = from_norm.to_string_lossy().to_string();
        let to_norm = common::normalize_path(to);
        let to_norm_str = to_norm.to_string_lossy().to_string();

        {
            let file = File::open(&self.path).context("Failed to open archive")?;
            let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;

            let mut writer = zip::ZipWriter::new(temp_file.as_file_mut());

            for i in 0..archive.len() {
                let (new_name, changed) = {
                    let zip_file = archive.by_index(i).context("Failed to get zip index")?;
                    let name = zip_file.name();
                    let name_raw = name.to_string();
                    let name_norm = common::normalize_path(name);
                    let name_norm_str = name_norm.to_string_lossy().to_string();

                    if name_norm_str == from_norm_str {
                        let mut nn = to_norm_str.clone();
                        // Preserve trailing slash if original had it
                        if name_raw.ends_with('/') && !nn.ends_with('/') {
                            nn.push('/');
                        }
                        (nn, true)
                    } else if name_norm_str.starts_with(&(from_norm_str.clone() + "/")) {
                        let mut nn = to_norm_str.clone() + &name_norm_str[from_norm_str.len()..];
                        if name_raw.ends_with('/') && !nn.ends_with('/') {
                            nn.push('/');
                        }
                        (nn, true)
                    } else {
                        (name_raw, false)
                    }
                };

                let mut zip_file = archive.by_index(i).context("Failed to get zip index")?;
                if changed {
                    let options = SimpleFileOptions::default()
                        .last_modified_time(zip_file.last_modified().unwrap_or_else(|| {
                            zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap()
                        }))
                        .unix_permissions(zip_file.unix_mode().unwrap_or(0o644));

                    writer.start_file(new_name, options)?;
                    std::io::copy(&mut zip_file, &mut writer)?;
                } else {
                    writer.raw_copy_file(zip_file)?;
                }
            }
            writer.finish()?;
        }

        std::fs::rename(temp_path, &self.path)?;
        Ok(())
    }
}
