use super::ArchiveFormat;
use super::common::{self, ArchiveEntryMetadata};
use crate::fs::fs_provider::TaskProgressContext;
use crate::fs::utils::mode_to_attributes;
use anyhow::{Context, Result};
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

    fn zip_dt_to_system_time(dt: zip::DateTime) -> std::time::SystemTime {
        use chrono::{TimeZone, Utc};
        let dt = Utc
            .with_ymd_and_hms(
                dt.year().into(),
                dt.month().into(),
                dt.day().into(),
                dt.hour().into(),
                dt.minute().into(),
                dt.second().into(),
            )
            .single()
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());
        dt.into()
    }

    fn get_unix_mode<R: std::io::Read>(zip_file: &zip::read::ZipFile<R>) -> Option<u32> {
        zip_file.unix_mode().filter(|&m| m != 0)
    }

    fn get_metadata_mode(metadata: &std::fs::Metadata) -> u32 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode()
        }
        #[cfg(not(unix))]
        {
            let _ = metadata;
            0o644
        }
    }

    fn system_time_to_zip_dt(t: SystemTime) -> zip::DateTime {
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
        .unwrap_or_else(|_| zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap())
    }

    fn has_entry(&self, name: &str) -> bool {
        if !self.path.exists() {
            return false;
        }
        let Ok(file) = File::open(&self.path) else {
            return false;
        };
        let Ok(mut archive) = zip::ZipArchive::new(file) else {
            return false;
        };
        archive.by_name(name).is_ok()
    }

    fn rewrite_all_entries<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(
            &mut zip::write::ZipWriter<&mut std::fs::File>,
            zip::read::ZipFile<'_, std::fs::File>,
        ) -> Result<()>,
    {
        let parent = self.path.parent().unwrap_or(Path::new("."));

        let mut temp_file = match tempfile::NamedTempFile::new_in(parent) {
            Ok(t) => t,
            Err(e) => tempfile::NamedTempFile::new().map_err(|err| {
                anyhow::anyhow!("Failed to create zip temp file ({e}; fallback {err})")
            })?,
        };
        let temp_path = temp_file.path().to_path_buf();
        let temp_in_parent = temp_path.parent() == Some(parent);

        {
            let file = File::open(&self.path).context("Failed to open archive")?;
            let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;
            let mut writer = zip::ZipWriter::new(temp_file.as_file_mut());

            for i in 0..archive.len() {
                let zip_file = archive.by_index(i).context("Failed to get zip index")?;
                f(&mut writer, zip_file)?;
            }
            writer.finish()?;
        }

        // Flush to disk and fully close all handles before installing the new
        // archive (some filesystems refuse to rename a file that is still open).
        temp_file.as_file().sync_all().ok();
        temp_file
            .keep()
            .context("Failed to finalize zip temp file")?;

        if temp_in_parent && std::fs::rename(&temp_path, &self.path).is_ok() {
            return Ok(());
        }

        // Fallback: replace the archive in place. Only write access to the
        // archive file itself is required, so this works on mounts that reject
        // rename-over-existing (e.g. vboxsf shared folders).
        let res = common::replace_file_in_place(temp_path.as_path(), self.path.as_path())
            .context("Failed to replace zip archive");
        common::remove_or_truncate_temp(&temp_path);
        res
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
            let modified = file
                .last_modified()
                .map_or(SystemTime::UNIX_EPOCH, Self::zip_dt_to_system_time);

            let unix_mode = Self::get_unix_mode(&file);
            let attributes = if let Some(mode) = unix_mode {
                mode_to_attributes(mode, is_dir, false)
            } else if is_dir {
                "drwxr-xr-x".to_string()
            } else {
                "-rw-r--r--".to_string()
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
        // Grow the buffer naturally instead of pre-reserving the central
        // directory's claimed size, which is untrusted and could be absurdly
        // large (e.g. a hostile archive claiming 4 GB).
        let mut buffer = Vec::new();
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
            let name = zip_file.name().to_string();
            let is_dir = false;
            let size = zip_file.size();
            let mode = Self::get_unix_mode(&zip_file).or(Some(0o100_644));
            let mtime = zip_file
                .last_modified()
                .map_or(SystemTime::UNIX_EPOCH, Self::zip_dt_to_system_time);

            let opts = common::ExtractOptions {
                src_str,
                dest,
                is_dir: false,
                progress,
            };

            common::handle_extraction_entry(
                &mut zip_file,
                &common::ExtractionEntryMetadata {
                    name_raw: &name,
                    is_dir,
                    is_symlink: false,
                    size,
                    mtime: Some(mtime),
                    mode,
                },
                &opts,
                &mut dir_mtimes,
                &mut last_update,
            )?;
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

            let name = zip_file.name().to_string();
            let is_dir = zip_file.is_dir() || name.ends_with('/');
            let size = zip_file.size();
            let mode = Self::get_unix_mode(&zip_file).or(if is_dir {
                Some(0o040_755)
            } else {
                Some(0o100_644)
            });
            let mtime = zip_file
                .last_modified()
                .map_or(SystemTime::UNIX_EPOCH, Self::zip_dt_to_system_time);

            common::handle_extraction_entry(
                &mut zip_file,
                &common::ExtractionEntryMetadata {
                    name_raw: &name,
                    is_dir,
                    is_symlink: false, // zip-rs doesn't easily support symlinks here
                    size,
                    mtime: Some(mtime),
                    mode,
                },
                &opts,
                &mut dir_mtimes,
                &mut last_update,
            )?;
        }

        common::preserve_mtimes(dir_mtimes);

        let p = progress.processed_items.load(Ordering::Relaxed);
        let _ = progress.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateProgress {
                task_id: progress.id,
                processed: p,
                total: 0,
            },
        ));

        Ok(())
    }

    fn do_add_files_and_directories(
        &self,
        files: &[(&Path, &str)],
        directories: &[(&str, Option<SystemTime>)],
    ) -> Result<()> {
        let mut dest_names: Vec<String> = Vec::with_capacity(files.len() + directories.len());
        for (_, dest) in files {
            let dest_norm = common::normalize_path(dest).to_string_lossy().to_string();
            dest_names.push(dest_norm);
        }
        for (dest, _) in directories {
            let mut dest_str = dest.to_string();
            if !dest_str.ends_with('/') {
                dest_str.push('/');
            }
            let dest_norm = common::normalize_path(&dest_str)
                .to_string_lossy()
                .to_string();
            dest_names.push(dest_norm);
        }

        // If any destination already exists, drop the old entries first so the
        // appended ones replace them.
        if dest_names.iter().any(|d| self.has_entry(d)) {
            let skip_set: std::collections::HashSet<&String> = dest_names.iter().collect();
            self.rewrite_all_entries(|writer, zip_file| {
                let name_norm = common::normalize_path(zip_file.name())
                    .to_string_lossy()
                    .to_string();
                if !skip_set.contains(&name_norm) {
                    writer.raw_copy_file(zip_file)?;
                }
                Ok(())
            })?;
        }

        // Append all entries in a single session.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .or_else(|_| {
                let f = File::create(&self.path)?;
                zip::ZipWriter::new(f).finish()?;
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&self.path)
            })?;

        let mut writer =
            zip::ZipWriter::new_append(file).context("Failed to open zip for appending")?;

        for (dest, mtime) in directories {
            let dest_norm = common::normalize_path(dest).to_string_lossy().to_string();
            let options = SimpleFileOptions::default()
                .last_modified_time(mtime.map_or_else(
                    || Self::system_time_to_zip_dt(SystemTime::now()),
                    Self::system_time_to_zip_dt,
                ))
                .unix_permissions(0o755);
            writer.add_directory(dest_norm, options)?;
        }
        for (src, dest) in files {
            let mut src_file = File::open(src).context("Failed to open source file")?;
            let metadata = src_file.metadata()?;
            let options = SimpleFileOptions::default()
                .last_modified_time(metadata.modified().ok().map_or_else(
                    || zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap(),
                    Self::system_time_to_zip_dt,
                ))
                .unix_permissions(Self::get_metadata_mode(&metadata));
            writer.start_file(dest, options)?;
            std::io::copy(&mut src_file, &mut writer)?;
        }
        writer.finish()?;
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
        let path_norm = common::normalize_path(path_str);
        let path_norm_str = path_norm.to_string_lossy().to_string();

        self.rewrite_all_entries(|writer, zip_file| {
            let name = zip_file.name();
            let name_norm = common::normalize_path(name);
            let name_norm_str = name_norm.to_string_lossy().to_string();

            let should_delete = name_norm_str == path_norm_str
                || name_norm_str.starts_with(&(path_norm_str.clone() + "/"));

            if !should_delete {
                writer.raw_copy_file(zip_file)?;
            }
            Ok(())
        })
    }

    /// Adds a file to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten or the source file cannot be read.
    fn add_file(&self, src: &Path, dest_in_archive: &str) -> Result<()> {
        let dest_norm = common::normalize_path(dest_in_archive);
        let dest_norm_str = dest_norm.to_string_lossy().to_string();

        if !self.has_entry(&dest_norm_str) {
            // ... (optimized append logic) ...
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.path)
                .or_else(|_| {
                    let f = File::create(&self.path)?;
                    zip::ZipWriter::new(f).finish()?;
                    std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&self.path)
                })?;

            let mut writer =
                zip::ZipWriter::new_append(file).context("Failed to open zip for appending")?;
            let mut src_file = File::open(src).context("Failed to open source file")?;
            let metadata = src_file.metadata()?;
            let options = SimpleFileOptions::default()
                .last_modified_time(metadata.modified().ok().map_or_else(
                    || zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap(),
                    Self::system_time_to_zip_dt,
                ))
                .unix_permissions(Self::get_metadata_mode(&metadata));

            writer.start_file(dest_in_archive, options)?;
            std::io::copy(&mut src_file, &mut writer)?;
            writer.finish()?;
            return Ok(());
        }

        // Fallback: full rewrite for replacement using rewrite_all_entries
        self.rewrite_all_entries(|writer, zip_file| {
            let name = zip_file.name();
            let name_norm = common::normalize_path(name);
            let name_norm_str = name_norm.to_string_lossy().to_string();

            if name_norm_str != dest_norm_str {
                writer.raw_copy_file(zip_file)?;
            }
            Ok(())
        })?;

        // Add the new file to the rewritten archive
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)?;
        let mut writer = zip::ZipWriter::new_append(file)?;
        let mut src_file = File::open(src)?;
        let metadata = src_file.metadata()?;
        let options = SimpleFileOptions::default()
            .last_modified_time(metadata.modified().ok().map_or_else(
                || zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap(),
                Self::system_time_to_zip_dt,
            ))
            .unix_permissions(Self::get_metadata_mode(&metadata));

        writer.start_file(dest_in_archive, options)?;
        std::io::copy(&mut src_file, &mut writer)?;
        writer.finish()?;
        Ok(())
    }

    /// Adds a directory to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn add_directory(&self, dest_in_archive: &str, mtime: Option<SystemTime>) -> Result<()> {
        let mut dest_str = dest_in_archive.to_string();
        if !dest_str.ends_with('/') {
            dest_str.push('/');
        }
        let dest_norm = common::normalize_path(&dest_str);
        let dest_norm_str = dest_norm.to_string_lossy().to_string();

        if !self.has_entry(&dest_norm_str) {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.path)
                .or_else(|_| {
                    let f = File::create(&self.path)?;
                    zip::ZipWriter::new(f).finish()?;
                    std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&self.path)
                })?;

            let mut writer =
                zip::ZipWriter::new_append(file).context("Failed to open zip for appending")?;
            let options = SimpleFileOptions::default()
                .last_modified_time(mtime.map_or_else(
                    || Self::system_time_to_zip_dt(SystemTime::now()),
                    Self::system_time_to_zip_dt,
                ))
                .unix_permissions(0o755);

            writer.add_directory(dest_norm_str, options)?;
            writer.finish()?;
            return Ok(());
        }

        // Fallback: full rewrite for replacement
        self.rewrite_all_entries(|writer, zip_file| {
            let name = zip_file.name();
            let name_norm = common::normalize_path(name);
            let name_norm_str = name_norm.to_string_lossy().to_string();

            if name_norm_str != dest_norm_str {
                writer.raw_copy_file(zip_file)?;
            }
            Ok(())
        })?;

        // Add the directory to the rewritten archive
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)?;
        let mut writer = zip::ZipWriter::new_append(file)?;
        let options = SimpleFileOptions::default()
            .last_modified_time(mtime.map_or_else(
                || Self::system_time_to_zip_dt(SystemTime::now()),
                Self::system_time_to_zip_dt,
            ))
            .unix_permissions(0o755);

        writer.add_directory(dest_norm_str, options)?;
        writer.finish()?;
        Ok(())
    }

    /// Sets the modified time of a file or directory within the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn set_modified_time(&self, path: &str, mtime: SystemTime) -> Result<()> {
        let path_norm = common::normalize_path(path);
        let path_norm_str = path_norm.to_string_lossy().to_string();

        self.rewrite_all_entries(|writer, mut zip_file| {
            let name = zip_file.name().to_string();
            let name_norm = common::normalize_path(&name);
            let name_norm_str = name_norm.to_string_lossy().to_string();

            if name_norm_str == path_norm_str {
                let options = SimpleFileOptions::default()
                    .last_modified_time(Self::system_time_to_zip_dt(mtime))
                    .unix_permissions(zip_file.unix_mode().unwrap_or(0o644));

                writer.start_file(name, options)?;
                std::io::copy(&mut zip_file, writer)?;
            } else {
                writer.raw_copy_file(zip_file)?;
            }
            Ok(())
        })
    }

    /// Renames a file or directory within the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn rename_file(&self, from: &str, to: &str) -> Result<()> {
        let from_norm = common::normalize_path(from);
        let from_norm_str = from_norm.to_string_lossy().to_string();
        let to_norm = common::normalize_path(to);
        let to_norm_str = to_norm.to_string_lossy().to_string();

        self.rewrite_all_entries(|writer, mut zip_file| {
            let name_raw = zip_file.name().to_string();
            let name_norm = common::normalize_path(&name_raw);
            let name_norm_str = name_norm.to_string_lossy().to_string();

            let (new_name, changed) = if name_norm_str == from_norm_str {
                let mut nn = to_norm_str.clone();
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
            };

            if changed {
                let options = SimpleFileOptions::default()
                    .last_modified_time(zip_file.last_modified().unwrap_or_else(|| {
                        zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap()
                    }))
                    .unix_permissions(zip_file.unix_mode().unwrap_or(0o644));

                writer.start_file(new_name, options)?;
                std::io::copy(&mut zip_file, writer)?;
            } else {
                writer.raw_copy_file(zip_file)?;
            }
            Ok(())
        })
    }

    /// Adds multiple files to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if any file cannot be added or the archive cannot be rewritten.
    fn add_files(&self, files: &[(&Path, &str)]) -> Result<()> {
        self.do_add_files_and_directories(files, &[])
    }

    /// Adds multiple files and directories to the archive in a single operation.
    ///
    /// # Errors
    ///
    /// Returns an error if any entry cannot be added or the archive cannot be rewritten.
    fn add_files_and_directories(
        &self,
        files: &[(&Path, &str)],
        directories: &[(&str, Option<SystemTime>)],
    ) -> Result<()> {
        self.do_add_files_and_directories(files, directories)
    }
}
