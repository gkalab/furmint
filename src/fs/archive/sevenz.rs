use super::common::{self, ArchiveEntryMetadata};
use super::{ArchiveFormat, ScanResult};
use crate::fs::fs_provider::TaskProgressContext;
use anyhow::{Context, Result};
use sevenz_rust2::{ArchiveEntry, ArchiveReader, ArchiveWriter, NtTime, Password};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::SystemTime;

pub struct SevenZHandler {
    path: PathBuf,
}

impl SevenZHandler {
    /// Creates a new `SevenZHandler` for the archive at `path`.
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

    fn open_reader(&self) -> Result<ArchiveReader<File>> {
        ArchiveReader::open(&self.path, Password::empty())
            .map_err(|e| anyhow::anyhow!("Failed to read 7z archive {}: {e}", self.path.display()))
    }

    /// Streams an archive entry into the writer, keeping memory usage low.
    fn push_entry(
        writer: &mut ArchiveWriter<File>,
        entry: &ArchiveEntry,
        data: &mut dyn Read,
    ) -> Result<(), sevenz_rust2::Error> {
        let cloned = entry.clone();
        if cloned.is_directory || cloned.size == 0 {
            writer.push_archive_entry(cloned, None::<&mut dyn Read>)?;
        } else {
            writer.push_archive_entry(cloned, Some(data))?;
        }
        Ok(())
    }

    /// Rewrites the archive by streaming every entry through `transform` and then
    /// invoking `after` so the caller can append new entries before finalizing.
    ///
    /// The output is written to a temporary file; a same-directory temp file is
    /// atomically renamed over the original once all handles are closed. When the
    /// archive's directory rejects that (e.g. vboxsf shared folders), the new
    /// archive is copied over the original in place instead.
    fn rewrite_and_finish<F, A>(&self, mut transform: F, after: A) -> Result<()>
    where
        F: FnMut(
            &ArchiveEntry,
            &mut dyn Read,
            &mut ArchiveWriter<File>,
        ) -> std::result::Result<(), sevenz_rust2::Error>,
        A: FnOnce(&mut ArchiveWriter<File>) -> std::result::Result<(), sevenz_rust2::Error>,
    {
        let parent = self.path.parent().unwrap_or(Path::new("."));

        // Build the new archive in a temp file next to the original when possible
        // so it can be installed atomically; otherwise use the system temp dir
        // and install it with an in-place copy below.
        let temp_file = match tempfile::NamedTempFile::new_in(parent) {
            Ok(t) => t,
            Err(e) => tempfile::NamedTempFile::new().map_err(|err| {
                anyhow::anyhow!("Failed to create 7z temp file ({e}; fallback {err})")
            })?,
        };
        let temp_path = temp_file.path().to_path_buf();
        let temp_in_parent = temp_path.parent() == Some(parent);

        {
            let mut reader = self.open_reader()?;
            let temp_handle = temp_file.reopen()?;
            let mut writer = ArchiveWriter::new(temp_handle)
                .map_err(|e| anyhow::anyhow!("Failed to create 7z writer: {e}"))?;

            reader
                .for_each_entries(|entry, data| {
                    transform(entry, data, &mut writer)?;
                    Ok(true)
                })
                .map_err(|e| anyhow::anyhow!("Failed to iterate 7z entries: {e}"))?;

            after(&mut writer).map_err(|e| anyhow::anyhow!("Failed to add 7z entries: {e}"))?;

            writer.finish().context("Failed to finish 7z writer")?;
        }

        // Flush to disk and fully close all handles before installing the new
        // archive (some filesystems refuse to rename a file that is still open).
        temp_file.as_file().sync_all().ok();
        temp_file
            .keep()
            .context("Failed to finalize 7z temp file")?;

        if temp_in_parent && std::fs::rename(&temp_path, &self.path).is_ok() {
            return Ok(());
        }

        // Fallback: replace the archive in place. Only write access to the
        // archive file itself is required, so this works on mounts that reject
        // rename-over-existing (e.g. vboxsf shared folders).
        let res = common::replace_file_in_place(temp_path.as_path(), self.path.as_path())
            .context("Failed to replace 7z archive");
        common::remove_or_truncate_temp(&temp_path);
        res
    }

    fn set_entry_mtime(entry: &mut ArchiveEntry, mtime: SystemTime) {
        entry.last_modified_date = NtTime::try_from(mtime).unwrap_or_else(|_| NtTime::now());
        entry.has_last_modified_date = true;
    }

    /// Interprets a stored 7z file attribute as a Unix mode when it carries
    /// file-type bits (p7zip writes the full `st_mode` when the archive's host
    /// OS is Unix, including `S_IFREG`/`S_IFDIR` in the high nibble).
    fn unix_mode(attrs: u32) -> Option<u32> {
        if attrs & 0xF000 != 0 {
            Some(attrs & 0xFFFF)
        } else {
            None
        }
    }

    fn do_scan(&self) -> Result<ScanResult> {
        let reader = self.open_reader()?;
        let files = reader.archive().files.clone();

        let mut entries_map = HashMap::new();
        let mut tree_map = HashMap::new();

        for file in &files {
            let path = common::normalize_path(file.name());
            let is_dir = file.is_directory;
            let size = if is_dir { None } else { Some(file.size) };
            let modified = if file.has_last_modified_date {
                Some(SystemTime::from(file.last_modified_date))
            } else {
                None
            };
            let attributes = if file.has_windows_attributes
                && let Some(mode) = Self::unix_mode(file.windows_attributes)
            {
                crate::fs::utils::mode_to_attributes(mode, is_dir, false)
            } else if is_dir {
                "drwxr-xr-x".to_string()
            } else if file.has_windows_attributes && file.windows_attributes & 0x1 != 0 {
                "-r--r--r--".to_string()
            } else {
                "-rw-r--r--".to_string()
            };

            common::add_to_tree(
                path,
                ArchiveEntryMetadata {
                    is_dir,
                    is_symlink: false,
                    size,
                    modified,
                    attributes,
                    position: None,
                },
                &mut entries_map,
                &mut tree_map,
            );
        }

        Ok((entries_map, common::finalize_tree(tree_map)))
    }

    fn do_read_file(&self, path_str: &str) -> Result<Vec<u8>> {
        let mut reader = self.open_reader()?;
        reader
            .read_file(path_str)
            .with_context(|| format!("File not found in 7z archive: {path_str}"))
    }

    fn do_extract(
        &self,
        src_str: &str,
        dest: &Path,
        is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        let mut reader = self.open_reader()?;

        let is_root = src_str.is_empty() || src_str == ".";
        let mut dir_mtimes = Vec::new();
        let mut last_update = std::time::Instant::now();

        let opts = common::ExtractOptions {
            src_str,
            dest,
            is_dir,
            progress,
        };

        reader
            .for_each_entries(|entry, data| {
                if progress.cancel.load(Ordering::Relaxed) {
                    return Ok(false);
                }

                let name = entry.name().replace('\\', "/");
                let name_trimmed = name.trim_end_matches('/').to_string();
                let should_extract = is_root
                    || name_trimmed == src_str
                    || name_trimmed.starts_with(&format!("{src_str}/"));
                if !should_extract {
                    return Ok(true);
                }

                let mtime = if entry.has_last_modified_date {
                    Some(SystemTime::from(entry.last_modified_date))
                } else {
                    None
                };
                let mode = if entry.is_directory {
                    Some(Self::unix_mode(entry.windows_attributes).unwrap_or(0o040_755))
                } else {
                    Some(Self::unix_mode(entry.windows_attributes).unwrap_or(0o100_644))
                };

                common::handle_extraction_entry(
                    data,
                    &common::ExtractionEntryMetadata {
                        name_raw: entry.name(),
                        is_dir: entry.is_directory,
                        is_symlink: false,
                        size: entry.size,
                        mtime,
                        mode,
                    },
                    &opts,
                    &mut dir_mtimes,
                    &mut last_update,
                )
                .map_err(|e| sevenz_rust2::Error::Other(e.to_string().into()))?;
                Ok(true)
            })
            .map_err(|e| anyhow::anyhow!("7z extraction failed: {e}"))?;

        common::preserve_mtimes(dir_mtimes);

        let p = progress.processed_items.load(Ordering::Relaxed);
        let _ = progress
            .tx
            .send(crate::tasks::TaskEvent::UpdateProgress(progress.id, p, 0));

        Ok(())
    }

    fn do_delete_file(&self, path_str: &str) -> Result<()> {
        let path_norm = common::normalize_path(path_str);
        let path_norm_str = path_norm.to_string_lossy().to_string();

        self.rewrite_and_finish(
            |entry, data, writer| {
                let name_norm = common::normalize_path(entry.name());
                let name_norm_str = name_norm.to_string_lossy().to_string();

                let should_delete = name_norm_str == path_norm_str
                    || name_norm_str.starts_with(&(path_norm_str.clone() + "/"));

                if !should_delete {
                    Self::push_entry(writer, entry, data)?;
                }
                Ok(())
            },
            |_: &mut ArchiveWriter<File>| Ok(()),
        )
    }

    fn do_add_file(&self, src: &Path, dest_in_archive: &str) -> Result<()> {
        self.do_add_entries(&[(src, dest_in_archive)], &[])
    }

    fn do_add_files(&self, files: &[(&Path, &str)]) -> Result<()> {
        self.do_add_entries(files, &[])
    }

    fn do_add_directory(&self, dest_in_archive: &str, mtime: Option<SystemTime>) -> Result<()> {
        self.do_add_entries(&[], &[(dest_in_archive, mtime)])
    }

    fn do_add_entries(
        &self,
        files: &[(&Path, &str)],
        directories: &[(&str, Option<SystemTime>)],
    ) -> Result<()> {
        let mut prepared_files: Vec<(PathBuf, String, Option<SystemTime>)> = Vec::new();
        let mut prepared_dirs: Vec<(String, SystemTime)> = Vec::new();
        let mut skip: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (src, dest) in files {
            let dest_norm = common::normalize_path(dest);
            let dest_norm_str = dest_norm.to_string_lossy().to_string();
            skip.insert(dest_norm_str.clone());
            let mtime = std::fs::metadata(src).and_then(|m| m.modified()).ok();
            prepared_files.push((src.to_path_buf(), dest_norm_str, mtime));
        }

        for (dest, mtime) in directories {
            let mut dest_str = dest.to_string();
            if !dest_str.ends_with('/') {
                dest_str.push('/');
            }
            let dest_norm = common::normalize_path(&dest_str);
            let dest_norm_str = dest_norm.to_string_lossy().to_string();
            skip.insert(dest_norm_str.clone());
            prepared_dirs.push((dest_norm_str, mtime.unwrap_or_else(SystemTime::now)));
        }

        self.rewrite_and_finish(
            |entry, data, writer| {
                let name_norm = common::normalize_path(entry.name());
                let name_norm_str = name_norm.to_string_lossy().to_string();

                if !skip.contains(&name_norm_str) {
                    Self::push_entry(writer, entry, data)?;
                }
                Ok(())
            },
            |writer| {
                for (dest, mtime) in prepared_dirs {
                    let mut entry = ArchiveEntry::new_directory(&dest);
                    Self::set_entry_mtime(&mut entry, mtime);
                    #[cfg(unix)]
                    {
                        entry.windows_attributes = 0o040_755;
                        entry.has_windows_attributes = true;
                    }
                    writer.push_archive_entry(entry, None::<&mut dyn Read>)?;
                }
                for (src, dest, mtime) in prepared_files {
                    let mut entry = ArchiveEntry::from_path(&src, dest);
                    if let Some(mtime) = mtime {
                        Self::set_entry_mtime(&mut entry, mtime);
                    }
                    #[cfg(unix)]
                    if let Ok(meta) = std::fs::metadata(&src) {
                        use std::os::unix::fs::PermissionsExt;
                        entry.windows_attributes = meta.permissions().mode();
                        entry.has_windows_attributes = true;
                    }
                    let file = File::open(&src).map_err(|e| {
                        sevenz_rust2::Error::Io(e, src.to_string_lossy().to_string().into())
                    })?;
                    writer.push_archive_entry(entry, Some(file))?;
                }
                Ok(())
            },
        )
    }

    fn do_set_modified_time(&self, path: &str, mtime: SystemTime) -> Result<()> {
        let path_norm = common::normalize_path(path);
        let path_norm_str = path_norm.to_string_lossy().to_string();

        self.rewrite_and_finish(
            |entry, data, writer| {
                let name_norm = common::normalize_path(entry.name());
                let name_norm_str = name_norm.to_string_lossy().to_string();

                if name_norm_str == path_norm_str {
                    let mut cloned = entry.clone();
                    Self::set_entry_mtime(&mut cloned, mtime);
                    if cloned.is_directory || cloned.size == 0 {
                        writer.push_archive_entry(cloned, None::<&mut dyn Read>)?;
                    } else {
                        writer.push_archive_entry(cloned, Some(data))?;
                    }
                } else {
                    Self::push_entry(writer, entry, data)?;
                }
                Ok(())
            },
            |_: &mut ArchiveWriter<File>| Ok(()),
        )
    }

    fn do_rename_file(&self, from: &str, to: &str) -> Result<()> {
        let from_norm = common::normalize_path(from);
        let from_norm_str = from_norm.to_string_lossy().to_string();
        let to_norm = common::normalize_path(to);
        let to_norm_str = to_norm.to_string_lossy().to_string();

        self.rewrite_and_finish(
            |entry, data, writer| {
                let name_raw = entry.name().to_string();
                let name_norm = common::normalize_path(&name_raw);
                let name_norm_str = name_norm.to_string_lossy().to_string();

                let (new_name, changed) = if name_norm_str == from_norm_str {
                    let mut nn = to_norm_str.clone();
                    if name_raw.ends_with('/') && !nn.ends_with('/') {
                        nn.push('/');
                    }
                    (nn, true)
                } else if name_norm_str.starts_with(&(from_norm_str.clone() + "/")) {
                    let suffix = &name_norm_str[from_norm_str.len()..];
                    let mut nn = to_norm_str.clone() + suffix;
                    if name_raw.ends_with('/') && !nn.ends_with('/') {
                        nn.push('/');
                    }
                    (nn, true)
                } else {
                    (String::new(), false)
                };

                if changed {
                    let mut cloned = entry.clone();
                    cloned.name = new_name;
                    if cloned.is_directory || cloned.size == 0 {
                        writer.push_archive_entry(cloned, None::<&mut dyn Read>)?;
                    } else {
                        writer.push_archive_entry(cloned, Some(data))?;
                    }
                } else {
                    Self::push_entry(writer, entry, data)?;
                }
                Ok(())
            },
            |_: &mut ArchiveWriter<File>| Ok(()),
        )
    }
}

impl ArchiveFormat for SevenZHandler {
    /// Scans the archive and returns its contents.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be read.
    fn scan(&self) -> Result<ScanResult> {
        self.do_scan()
    }

    /// Reads a file from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_file(&self, path_str: &str) -> Result<Vec<u8>> {
        self.do_read_file(path_str)
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
        self.do_extract(src_str, dest, is_dir, progress)
    }

    /// Deletes a file or directory from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn delete_file(&self, path_str: &str) -> Result<()> {
        self.do_delete_file(path_str)
    }

    /// Adds a file from the local filesystem to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten or the source cannot be read.
    fn add_file(&self, src: &Path, dest_in_archive: &str) -> Result<()> {
        self.do_add_file(src, dest_in_archive)
    }

    /// Adds multiple files to the archive in a single rewrite.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten or a source cannot be read.
    fn add_files(&self, files: &[(&Path, &str)]) -> Result<()> {
        self.do_add_files(files)
    }

    /// Adds a directory to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn add_directory(&self, dest_in_archive: &str, mtime: Option<SystemTime>) -> Result<()> {
        self.do_add_directory(dest_in_archive, mtime)
    }

    /// Adds multiple files and directories to the archive in a single rewrite.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten or a source cannot be read.
    fn add_files_and_directories(
        &self,
        files: &[(&Path, &str)],
        directories: &[(&str, Option<SystemTime>)],
    ) -> Result<()> {
        self.do_add_entries(files, directories)
    }

    /// Sets the modified time of a file or directory within the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn set_modified_time(&self, path: &str, mtime: SystemTime) -> Result<()> {
        self.do_set_modified_time(path, mtime)
    }

    /// Renames a file or directory within the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn rename_file(&self, from: &str, to: &str) -> Result<()> {
        self.do_rename_file(from, to)
    }
}
