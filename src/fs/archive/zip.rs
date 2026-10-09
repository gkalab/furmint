use super::ArchiveFormat;
use super::common::{self, ArchiveEntryMetadata};
use super::{NewEntry, PlannedDir, PlannedFile};
use crate::fs::fs_provider::TaskProgressContext;
use crate::fs::utils::mode_to_attributes;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, Write};
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

    /// Canonical form of an entry name, used to match archive entries against
    /// caller-supplied paths.
    fn entry_key(name: &str) -> String {
        common::normalize_path(name).to_string_lossy().into_owned()
    }

    /// Keys of every entry currently in the archive.
    ///
    /// The archive is opened and its central directory parsed exactly once, so
    /// classifying a batch of destinations stays linear instead of reopening the
    /// archive per name. A missing or unreadable archive simply has no entries.
    fn existing_entries(&self) -> HashSet<String> {
        let Ok(file) = File::open(&self.path) else {
            return HashSet::new();
        };
        let Ok(mut archive) = zip::ZipArchive::new(file) else {
            return HashSet::new();
        };
        let mut keys = HashSet::with_capacity(archive.len());
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                keys.insert(Self::entry_key(entry.name()));
            }
        }
        keys
    }

    /// Rewrites the archive, letting `copy_entries` decide what happens to each
    /// existing entry and then running `emit_new` against the same writer
    /// before the result is installed.
    ///
    /// Doing the drop and the replacement inside one pass matters: splitting it
    /// into a rewrite plus a follow-up append would leave the destination
    /// missing if the process died in between.
    fn rewrite_with<F, G>(&self, mut copy_entries: F, emit_new: G) -> Result<()>
    where
        F: FnMut(
            &mut zip::write::ZipWriter<&mut std::fs::File>,
            zip::read::ZipFile<'_, std::fs::File>,
        ) -> Result<()>,
        G: FnOnce(&mut zip::write::ZipWriter<&mut std::fs::File>) -> Result<()>,
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
                copy_entries(&mut writer, zip_file)?;
            }
            emit_new(&mut writer)?;
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

    /// [`Self::rewrite_with`] for rewrites that only rearrange existing entries.
    fn rewrite_all_entries<F>(&self, copy_entries: F) -> Result<()>
    where
        F: FnMut(
            &mut zip::write::ZipWriter<&mut std::fs::File>,
            zip::read::ZipFile<'_, std::fs::File>,
        ) -> Result<()>,
    {
        self.rewrite_with(copy_entries, |_| Ok(()))
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
        let dest = common::canonicalize_dest(dest);

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
                dest: &dest,
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
            dest: &dest,
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
        progress.tx.send(crate::tasks::UiEvent::Task(
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
        let planned: Vec<PlannedFile<'_>> = files
            .iter()
            .map(|(src, dest)| PlannedFile::from_file(dest, src))
            .collect();
        let planned_dirs: Vec<PlannedDir> = directories
            .iter()
            .map(|(dest, mtime)| PlannedDir::new(dest, *mtime))
            .collect();
        self.do_add_planned(&planned, &planned_dirs)
    }

    /// Opens the archive for appending, creating an empty one if it is missing.
    fn open_for_append(&self) -> Result<std::fs::File> {
        std::fs::OpenOptions::new()
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
            })
            .map_err(Into::into)
    }

    /// Writes the queued entries into an already-open writer.
    fn emit_planned<W: Write + Seek>(
        writer: &mut zip::ZipWriter<W>,
        files: &[PlannedFile<'_>],
        directories: &[PlannedDir],
    ) -> Result<()> {
        for dir in directories {
            let options = SimpleFileOptions::default()
                .last_modified_time(dir.mtime().map_or_else(
                    || Self::system_time_to_zip_dt(SystemTime::now()),
                    Self::system_time_to_zip_dt,
                ))
                .unix_permissions(0o755);
            writer.add_directory(dir.dest(), options)?;
        }

        for planned in files {
            let (mtime, mode) = planned.resolve_meta()?;
            let options = SimpleFileOptions::default()
                .last_modified_time(mtime.map_or_else(
                    || zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap(),
                    Self::system_time_to_zip_dt,
                ))
                .unix_permissions(mode);
            writer.start_file(planned.dest(), options)?;
            let mut src = planned.open()?;
            std::io::copy(&mut src, writer)?;
        }

        Ok(())
    }

    /// Adds all queued entries in a single session, replacing any existing
    /// entries with the same destination.
    fn do_add_planned(&self, files: &[PlannedFile<'_>], directories: &[PlannedDir]) -> Result<()> {
        let dest_names: Vec<String> = files
            .iter()
            .map(|f| Self::entry_key(f.dest()))
            .chain(directories.iter().map(|d| Self::entry_key(d.dest())))
            .collect();

        let existing = self.existing_entries();
        let replaced: HashSet<String> = dest_names
            .into_iter()
            .filter(|name| existing.contains(name))
            .collect();

        if replaced.is_empty() {
            // Nothing is being replaced: append in place so the untouched
            // entries never have to be re-read or rewritten.
            let mut writer = zip::ZipWriter::new_append(self.open_for_append()?)
                .context("Failed to open zip for appending")?;
            Self::emit_planned(&mut writer, files, directories)?;
            writer.finish()?;
            return Ok(());
        }

        // At least one destination already exists, so the archive has to be
        // rewritten. Drop the stale entries and emit the new ones in the *same*
        // pass: a rewrite followed by a separate append would leave the
        // destination missing if the process died in between.
        self.rewrite_with(
            |writer, zip_file| {
                if !replaced.contains(&Self::entry_key(zip_file.name())) {
                    writer.raw_copy_file(zip_file)?;
                }
                Ok(())
            },
            |writer| Self::emit_planned(writer, files, directories),
        )
    }

    fn do_add_entries(&self, entries: &[NewEntry]) -> Result<()> {
        let mut files: Vec<PlannedFile<'_>> = Vec::with_capacity(entries.len());
        let mut directories: Vec<PlannedDir> = Vec::new();

        for entry in entries {
            if let Some(data) = &entry.data {
                files.push(PlannedFile::from_memory(
                    &entry.dest,
                    data,
                    entry.mtime,
                    entry.mode,
                ));
            } else {
                directories.push(PlannedDir::new(&entry.dest, entry.mtime));
            }
        }

        self.do_add_planned(&files, &directories)
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
        let path_key = Self::entry_key(path_str);
        let path_prefix = format!("{path_key}/");

        self.rewrite_all_entries(|writer, zip_file| {
            let name_key = Self::entry_key(zip_file.name());

            if name_key != path_key && !name_key.starts_with(&path_prefix) {
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
        self.do_add_planned(&[PlannedFile::from_file(dest_in_archive, src)], &[])
    }

    /// Adds a directory to the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn add_directory(&self, dest_in_archive: &str, mtime: Option<SystemTime>) -> Result<()> {
        self.do_add_planned(&[], &[PlannedDir::new(dest_in_archive, mtime)])
    }

    /// Sets the modified time of a file or directory within the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be rewritten.
    fn set_modified_time(&self, path: &str, mtime: SystemTime) -> Result<()> {
        let path_key = Self::entry_key(path);

        self.rewrite_all_entries(|writer, zip_file| {
            if Self::entry_key(zip_file.name()) == path_key {
                // Touch the timestamp without re-encoding: the stored bytes are
                // copied verbatim, so the entry keeps the compression method it
                // was archived with instead of being run through the deflate
                // encoder again.
                writer.raw_copy_file_touch(zip_file, Self::system_time_to_zip_dt(mtime), None)?;
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
        let from_key = Self::entry_key(from);
        let from_prefix = format!("{from_key}/");
        let to_key = Self::entry_key(to);

        self.rewrite_all_entries(|writer, zip_file| {
            let name_raw = zip_file.name();
            let name_key = Self::entry_key(name_raw);

            let new_name = if name_key == from_key {
                Some(to_key.clone())
            } else {
                name_key
                    .strip_prefix(&from_prefix)
                    .map(|rest| format!("{to_key}/{rest}"))
            };

            let Some(raw_new_name) = new_name else {
                writer.raw_copy_file(zip_file)?;
                return Ok(());
            };

            // Zip marks a directory by its trailing slash, so keep the one the
            // source entry had.
            let mut new_name = raw_new_name.trim_matches('/').to_string();
            if name_raw.ends_with('/') {
                new_name.push('/');
            }

            // Rename by raw copy: the compressed bytes are reused verbatim and
            // the entry keeps the compression method it was archived with
            // instead of being decompressed and re-encoded.
            writer.raw_copy_file_rename(zip_file, new_name)?;
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

    /// Adds in-memory entries to the archive in a single append.
    ///
    /// # Errors
    ///
    /// Returns an error if any entry cannot be added or the archive cannot be rewritten.
    fn add_entries(&self, entries: &[NewEntry]) -> Result<()> {
        self.do_add_entries(entries)
    }
}
