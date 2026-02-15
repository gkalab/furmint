use crate::fs::fs_provider::FileSystemProvider;
use crate::fs::traits::TaskProgressContext;
use crate::fs::utils::FileEntry;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::TimeZone;
use filetime::{FileTime, set_file_mtime};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[derive(Clone)]
pub struct ArchiveFs {
    archive_path: PathBuf,
    // We map "internal path" -> FileEntry
    // The "internal path" should be relative to archive root, e.g. "folder/file.txt"
    entries: Arc<Mutex<HashMap<PathBuf, FileEntry>>>,
    // Store children for fast directory listing: "folder" -> ["folder/sub", "folder/file.txt"]
    tree: Arc<Mutex<HashMap<PathBuf, Vec<PathBuf>>>>,
    temp_tar: Arc<Mutex<Option<tempfile::NamedTempFile>>>,
}

impl ArchiveFs {
    pub fn new(path: &Path) -> Result<Self> {
        let entries = Arc::new(Mutex::new(HashMap::new()));
        let tree = Arc::new(Mutex::new(HashMap::new()));
        let temp_tar = Arc::new(Mutex::new(None));
        let fs = Self {
            archive_path: path.to_path_buf(),
            entries,
            tree,
            temp_tar,
        };

        fs.scan_archive()?;
        Ok(fs)
    }

    pub fn get_entry(&self, path: &Path) -> Option<FileEntry> {
        let entries = self.entries.lock().unwrap();
        entries.get(path).cloned()
    }

    fn scan_archive(&self) -> Result<()> {
        let ext = self
            .archive_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "zip" {
            let file = File::open(&self.archive_path).context("Failed to open archive")?;
            let reader = std::io::BufReader::new(file);
            self.scan_zip(reader)?;
        } else if ext == "tar" {
            let file = File::open(&self.archive_path).context("Failed to open archive")?;
            let reader = std::io::BufReader::new(file);
            self.scan_tar(reader)?;
        } else if ext == "gz" || ext == "tgz" || ext == "bz2" || ext == "tbz2" {
            let mut temp = tempfile::NamedTempFile::new()?;
            let cmd_name = if ext == "gz" || ext == "tgz" {
                "gzip"
            } else {
                "bzip2"
            };

            // Try system command first for performance
            let mut decompressed_via_system = false;
            if let Ok(mut child) = Command::new(cmd_name)
                .arg("-dc")
                .arg(&self.archive_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                && let Some(mut stdout) = child.stdout.take()
                && std::io::copy(&mut stdout, temp.as_file_mut()).is_ok()
                && child.wait().map(|s| s.success()).unwrap_or(false)
            {
                decompressed_via_system = true;
            }

            if !decompressed_via_system {
                // Seek back to start of temp if system command wrote something then failed
                temp.as_file_mut().seek(std::io::SeekFrom::Start(0))?;
                temp.as_file_mut().set_len(0)?; // Truncate

                let file = File::open(&self.archive_path)
                    .context("Failed to open archive for fallback")?;
                let reader = std::io::BufReader::new(file);
                if ext == "gz" || ext == "tgz" {
                    let mut decoder = flate2::read::GzDecoder::new(reader);
                    std::io::copy(&mut decoder, temp.as_file_mut())?;
                } else {
                    let mut decoder = bzip2::read::BzDecoder::new(reader);
                    std::io::copy(&mut decoder, temp.as_file_mut())?;
                }
            }

            // Seek back to start for scanning
            temp.as_file_mut().seek(std::io::SeekFrom::Start(0))?;

            // Scan the uncompressed tar
            let reader = std::io::BufReader::new(temp.as_file());
            self.scan_tar(reader)?;

            // Store the temp file for later access
            *self.temp_tar.lock().unwrap() = Some(temp);
        } else {
            return Err(anyhow::anyhow!("Unsupported archive format: {}", ext));
        }

        Ok(())
    }

    fn scan_zip<R: Read + std::io::Seek>(&self, reader: R) -> Result<()> {
        let mut archive = zip::ZipArchive::new(reader).context("Failed to read zip archive")?;
        let mut entries_map = HashMap::new();
        let mut tree_map: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let name = file.name().to_string();

            // Normalize path separators and trim trailing slashes
            let path_str = name.replace('\\', "/");
            let normalized_path_str = path_str.trim_end_matches('/');
            let path = PathBuf::from(normalized_path_str);

            let is_dir = file.is_dir() || name.ends_with('/');

            let size = file.size();
            let modified = Self::zip_dt_to_system_time(file.last_modified());

            let entry = FileEntry {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                is_dir,
                is_symlink: false,
                size: Some(size),
                modified: Some(modified),
                attributes: if is_dir {
                    "dr-xr-xr-x".to_string()
                } else {
                    "-r--r--r--".to_string()
                },
                selected: false,
                position: None,
            };

            entries_map.insert(path.clone(), entry);

            // Populate tree
            if let Some(parent) = path.parent() {
                let parent = if parent == Path::new("") {
                    Path::new(".").to_path_buf()
                } else {
                    parent.to_path_buf()
                };
                tree_map.entry(parent).or_default().insert(path.clone());

                // Ensure parent directories exist in the map even if not explicitly in zip
                let mut curr = path.clone();
                while let Some(p) = curr.parent() {
                    let p_norm = if p == Path::new("") {
                        Path::new(".")
                    } else {
                        p
                    };
                    if p_norm == Path::new(".") {
                        break;
                    }

                    // If parent entry doesn't exist, create implicit dir
                    if !entries_map.contains_key(p_norm) {
                        let implicit_entry = FileEntry {
                            name: p_norm
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                            is_dir: true,
                            is_symlink: false,
                            size: None,
                            modified: None,
                            attributes: "dr-xr-xr-x".to_string(),
                            selected: false,
                            position: None,
                        };
                        entries_map.insert(p_norm.to_path_buf(), implicit_entry);

                        if let Some(pp) = p_norm.parent() {
                            let pp_norm = if pp == Path::new("") {
                                Path::new(".")
                            } else {
                                pp
                            };
                            tree_map
                                .entry(pp_norm.to_path_buf())
                                .or_default()
                                .insert(p_norm.to_path_buf());
                        }
                    }
                    curr = p_norm.to_path_buf();
                }
            } else {
                // Root level item
                tree_map
                    .entry(Path::new(".").to_path_buf())
                    .or_default()
                    .insert(path);
            }
        }

        let mut locked_entries = self.entries.lock().unwrap();
        *locked_entries = entries_map;

        // Convert HashSet to Vec for tree
        let mut locked_tree = self.tree.lock().unwrap();
        for (k, v) in tree_map {
            let mut children: Vec<PathBuf> = v.into_iter().collect();
            children.sort();
            locked_tree.insert(k, children);
        }

        Ok(())
    }

    fn scan_tar<R: Read>(&self, reader: R) -> Result<()> {
        let mut archive = tar::Archive::new(reader);
        let mut entries_map = HashMap::new();
        let mut tree_map: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();

        for file in archive.entries()? {
            let file = file?;
            let path_owned = file.path()?.into_owned();
            // Normalize path separators and trim trailing slashes
            let path_str = path_owned.to_string_lossy().replace('\\', "/");
            let normalized_path_str = path_str.trim_end_matches('/');
            let path = PathBuf::from(normalized_path_str);

            let is_dir = file.header().entry_type().is_dir();
            let size = file.size();
            let modified = SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(file.header().mtime().unwrap_or(0));

            let position = file.raw_header_position();

            let entry = FileEntry {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                is_dir,
                is_symlink: file.header().entry_type().is_symlink(),
                size: Some(size),
                modified: Some(modified),
                attributes: if is_dir {
                    "dr-xr-xr-x".to_string()
                } else {
                    "-r--r--r--".to_string()
                },
                selected: false,
                position: Some(position),
            };

            entries_map.insert(path.clone(), entry);

            // Populate tree similar to zip
            if let Some(parent) = path.parent() {
                let parent = if parent == Path::new("") {
                    Path::new(".").to_path_buf()
                } else {
                    parent.to_path_buf()
                };
                tree_map.entry(parent).or_default().insert(path.clone());

                // Ensure parent directories exist
                let mut curr = path.clone();
                while let Some(p) = curr.parent() {
                    let p_norm = if p == Path::new("") {
                        Path::new(".")
                    } else {
                        p
                    };
                    if p_norm == Path::new(".") {
                        break;
                    }

                    if !entries_map.contains_key(p_norm) {
                        let implicit_entry = FileEntry {
                            name: p_norm
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                            is_dir: true,
                            is_symlink: false,
                            size: None,
                            modified: None,
                            attributes: "dr-xr-xr-x".to_string(),
                            selected: false,
                            position: None,
                        };
                        entries_map.insert(p_norm.to_path_buf(), implicit_entry);

                        if let Some(pp) = p_norm.parent() {
                            let pp_norm = if pp == Path::new("") {
                                Path::new(".")
                            } else {
                                pp
                            };
                            tree_map
                                .entry(pp_norm.to_path_buf())
                                .or_default()
                                .insert(p_norm.to_path_buf());
                        }
                    }
                    curr = p_norm.to_path_buf();
                }
            } else {
                tree_map
                    .entry(Path::new(".").to_path_buf())
                    .or_default()
                    .insert(path);
            }
        }

        let mut locked_entries = self.entries.lock().unwrap();
        *locked_entries = entries_map;

        let mut locked_tree = self.tree.lock().unwrap();
        for (k, v) in tree_map {
            let mut children: Vec<PathBuf> = v.into_iter().collect();
            children.sort();
            locked_tree.insert(k, children);
        }

        Ok(())
    }

    fn zip_dt_to_system_time(dt: Option<zip::DateTime>) -> SystemTime {
        if let Some(dt) = dt {
            chrono::Utc
                .with_ymd_and_hms(
                    dt.year() as i32,
                    dt.month() as u32,
                    dt.day() as u32,
                    dt.hour() as u32,
                    dt.minute() as u32,
                    dt.second() as u32,
                )
                .single()
                .map(SystemTime::from)
                .unwrap_or(SystemTime::UNIX_EPOCH)
        } else {
            SystemTime::UNIX_EPOCH
        }
    }
}

#[async_trait]
impl FileSystemProvider for ArchiveFs {
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };

        let search_path = if rel_path == Path::new("") {
            Path::new(".")
        } else {
            rel_path
        };

        let tree = self.tree.lock().unwrap();
        let entries_map = self.entries.lock().unwrap();

        let mut result = Vec::new();

        // Always add ".."
        if search_path != Path::new(".") {
            result.push(FileEntry {
                name: "..".to_string(),
                is_dir: true,
                is_symlink: false,
                size: None,
                modified: None,
                attributes: String::new(),
                selected: false,
                position: None,
            });
        }

        if let Some(children) = tree.get(search_path) {
            for child_path in children {
                if let Some(entry) = entries_map.get(child_path) {
                    result.push(entry.clone());
                }
            }
        }

        Ok(result)
    }

    fn create_dir(&self, _path: &Path) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn create_file(&self, _path: &Path) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn delete(&self, _path: &Path, _recursive: bool) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn rename(&self, _from: &Path, _to: &Path) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let path_str = rel_path.to_string_lossy().replace('\\', "/");
        let path_str = path_str.trim_end_matches('/');

        let file = File::open(&self.archive_path).context("Failed to open archive")?;
        let ext = self
            .archive_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "zip" {
            let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;
            let mut zip_file = archive.by_name(path_str).context("File not found in zip")?;
            let mut buffer = Vec::with_capacity(zip_file.size() as usize);
            zip_file.read_to_end(&mut buffer)?;
            Ok(buffer)
        } else if ext == "tar" || ext == "gz" || ext == "tgz" || ext == "bz2" || ext == "tbz2" {
            let reader = std::io::BufReader::new(file);
            if ext == "gz" || ext == "tgz" {
                let tar = flate2::read::GzDecoder::new(reader);
                Self::read_tar_file(tar, path_str)
            } else if ext == "bz2" || ext == "tbz2" {
                let tar = bzip2::read::BzDecoder::new(reader);
                Self::read_tar_file(tar, path_str)
            } else {
                Self::read_tar_file(reader, path_str)
            }
        } else {
            Err(anyhow::anyhow!("Unsupported archive format for reading"))
        }
    }

    fn read_file_at(&self, path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
        // Simple implementation: read all then chunk. Archives don't support random access well generally.
        let data = self.read_file(path)?;
        let start = offset as usize;
        if start >= data.len() {
            return Ok(Vec::new());
        }
        let end = (start + len).min(data.len());
        Ok(data[start..end].to_vec())
    }

    fn write_file(&self, _path: &Path, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn write_file_at(&self, _path: &Path, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("ArchiveFileSystem is read-only"))
    }

    fn display_prefix(&self) -> &str {
        ""
    }

    fn is_local(&self) -> bool {
        false
    }

    fn exists(&self, path: &Path) -> bool {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_norm = PathBuf::from(p_str.trim_end_matches('/'));
        let p = if p_norm == Path::new("") {
            Path::new(".")
        } else {
            &p_norm
        };
        self.entries.lock().unwrap().contains_key(p) || p == Path::new(".")
    }

    fn is_dir(&self, path: &Path) -> bool {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_norm = PathBuf::from(p_str.trim_end_matches('/'));
        let p = if p_norm == Path::new("") {
            Path::new(".")
        } else {
            &p_norm
        };

        if p == Path::new(".") {
            true
        } else {
            self.entries
                .lock()
                .unwrap()
                .get(p)
                .map(|e| e.is_dir)
                .unwrap_or(false)
        }
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        Ok(path.to_path_buf())
    }

    fn get_permissions(&self, _path: &Path) -> Option<u32> {
        Some(0o444) // Read only
    }

    fn set_permissions(&self, _path: &Path, _mode: u32) -> bool {
        false
    }

    fn get_modified_time(&self, path: &Path) -> Option<SystemTime> {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_norm = PathBuf::from(p_str.trim_end_matches('/'));
        let p = if p_norm == Path::new("") {
            Path::new(".")
        } else {
            &p_norm
        };

        if p == Path::new(".") {
            return None;
        }
        self.entries.lock().unwrap().get(p).and_then(|e| e.modified)
    }

    fn set_modified_time(&self, _path: &Path, _mtime: SystemTime) -> bool {
        false
    }

    fn context_key(&self) -> String {
        format!("archive:{}", self.archive_path.to_string_lossy())
    }

    fn display_path(&self, path: &Path) -> String {
        if path.has_root() {
            path.to_string_lossy().to_string()
        } else {
            format!("/{}", path.to_string_lossy())
        }
    }

    async fn calc_dir_size(&self, _path: &Path) -> anyhow::Result<u64> {
        Ok(0)
    }

    async fn download(
        &self,
        src: &Path,
        dest_fs: &dyn crate::fs::traits::FileSystem,
        dest: &Path,
        progress: &crate::fs::traits::TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        // Optimized download: extract directly if possible
        if !dest_fs.is_local() {
            return None;
        }

        let archive_path = self.archive_path.clone();
        let src = src.to_path_buf();
        let dest = dest.to_path_buf();
        let progress = progress.clone();
        let is_dir = self.is_dir(&src);
        let entries_map = self.entries.clone();
        let temp_tar = self.temp_tar.clone();

        Some(
            tokio::task::spawn_blocking(move || {
                let rel_src = if src.has_root() {
                    src.strip_prefix("/").unwrap_or(&src)
                } else {
                    &src
                };
                let src_str = rel_src.to_string_lossy().replace('\\', "/");
                let src_str = src_str.trim_end_matches('/').to_string();
                let is_root = src_str.is_empty() || src_str == ".";

                let mut last_update = std::time::Instant::now();
                let mut p;
                let mut dir_mtimes = Vec::new();

                let ext = archive_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let temp_tar_path = {
                    let lock = temp_tar.lock().unwrap();
                    lock.as_ref().map(|t| t.path().to_path_buf())
                };

                let effective_path = if ext == "zip" {
                    &archive_path
                } else {
                    temp_tar_path.as_ref().unwrap_or(&archive_path)
                };

                let file = File::open(effective_path).context("Failed to open archive")?;

                if ext == "zip" {
                    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;

                    // Optimized single file extraction
                    if !is_root
                        && let Ok(mut zip_file) = archive.by_name(&src_str)
                        && !zip_file.is_dir()
                    {
                        if let Some(parent) = dest.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut out = File::create(&dest)?;
                        std::io::copy(&mut zip_file, &mut out)?;
                        let size = zip_file.size();
                        progress
                            .processed_bytes
                            .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                        let p = progress
                            .processed_items
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                            + 1;
                        let _ = progress
                            .tx
                            .send(crate::tasks::TaskEvent::UpdateByteProgress(
                                progress.id,
                                progress
                                    .processed_bytes
                                    .load(std::sync::atomic::Ordering::Relaxed),
                                0,
                            ));
                        let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                            progress.id,
                            p,
                            0,
                        ));
                        return Ok(());
                    }

                    for i in 0..archive.len() {
                        if progress.cancel.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        let mut zip_file =
                            archive.by_index(i).context("Failed to get zip index")?;
                        let name_raw = zip_file.name().to_string();
                        let name = name_raw.replace('\\', "/");
                        let name = name.trim_end_matches('/');

                        let should_extract = if is_root {
                            true
                        } else {
                            name == src_str || name.starts_with(&format!("{}/", src_str))
                        };

                        if should_extract {
                            let rel_path = if is_root {
                                PathBuf::from(name)
                            } else {
                                Path::new(name)
                                    .strip_prefix(&src_str)
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or_else(|_| PathBuf::from(name))
                            };

                            let rel_name_str = rel_path.to_string_lossy().to_string();

                            if rel_name_str.is_empty() && zip_file.is_dir() {
                                continue;
                            }

                            let target = if rel_name_str.is_empty() {
                                dest.clone()
                            } else {
                                dest.join(&rel_name_str)
                            };

                            let mtime = Self::zip_dt_to_system_time(zip_file.last_modified());

                            if zip_file.is_dir() || name.ends_with('/') {
                                std::fs::create_dir_all(&target)?;
                                dir_mtimes.push((target, mtime));
                            } else {
                                if let Some(parent) = target.parent() {
                                    std::fs::create_dir_all(parent)?;
                                }
                                let mut out = File::create(&target)?;
                                std::io::copy(&mut zip_file, &mut out)?;
                                let size = zip_file.size();
                                progress
                                    .processed_bytes
                                    .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                                // Set modified time for file
                                let _ = set_file_mtime(&target, FileTime::from_system_time(mtime));
                            }

                            p = progress.processed_items.fetch_add(1, Ordering::Relaxed) + 1;

                            let now = std::time::Instant::now();
                            if p % 10 == 0
                                || now.duration_since(last_update)
                                    > std::time::Duration::from_millis(100)
                            {
                                let _ =
                                    progress.tx.send(crate::tasks::TaskEvent::UpdateCurrentFile(
                                        progress.id,
                                        rel_name_str,
                                    ));
                                let _ =
                                    progress
                                        .tx
                                        .send(crate::tasks::TaskEvent::UpdateByteProgress(
                                            progress.id,
                                            progress.processed_bytes.load(Ordering::Relaxed),
                                            0,
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
                    // Apply directory transitions in reverse order of depth to avoid spoiling
                    dir_mtimes.sort_by(|a, b| b.0.as_os_str().len().cmp(&a.0.as_os_str().len()));
                    for (dir, mtime) in dir_mtimes {
                        let _ = set_file_mtime(&dir, FileTime::from_system_time(mtime));
                    }

                    // Send final update for this provider
                    let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                        progress.id,
                        progress.processed_items.load(Ordering::Relaxed),
                        0,
                    ));
                } else {
                    // Tar implementation
                    // Try optimized single file extraction if we have the position
                    if !is_root && !is_dir {
                        let position = {
                            let entries = entries_map.lock().unwrap();
                            entries
                                .get(&PathBuf::from(&src_str))
                                .and_then(|e| e.position)
                        };

                        if let Some(pos) = position {
                            let mut file =
                                File::open(effective_path).context("Failed to open archive")?;
                            file.seek(std::io::SeekFrom::Start(pos))?;
                            let mut archive = tar::Archive::new(file);
                            let mut entries = archive.entries()?;
                            if let Some(Ok(mut entry)) = entries.next() {
                                // Double check it's the right file to be safe
                                let entry_path = entry.path()?.to_string_lossy().replace('\\', "/");
                                let entry_path = entry_path.trim_end_matches('/');
                                if entry_path == src_str {
                                    if let Some(parent) = dest.parent() {
                                        std::fs::create_dir_all(parent)?;
                                    }
                                    let mut out = File::create(&dest)?;
                                    std::io::copy(&mut entry, &mut out)?;
                                    let size = entry.size();
                                    progress
                                        .processed_bytes
                                        .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                                    let p = progress
                                        .processed_items
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                        + 1;
                                    let _ = progress.tx.send(
                                        crate::tasks::TaskEvent::UpdateProgress(progress.id, p, 0),
                                    );
                                    return Ok(());
                                }
                            }
                        }
                    }

                    // Fallback to system tar for performance or directory extraction
                    if Self::system_tar_extract(effective_path, &src_str, &dest, is_dir, &progress)
                        .is_ok()
                    {
                        return Ok(());
                    }

                    // Fallback to Rust implementation
                    let reader = std::io::BufReader::new(file);
                    if ext == "gz" || ext == "tgz" {
                        let tar = flate2::read::GzDecoder::new(reader);
                        Self::extract_tar(tar, &src_str, &dest, &progress)?;
                    } else if ext == "bz2" || ext == "tbz2" {
                        let tar = bzip2::read::BzDecoder::new(reader);
                        Self::extract_tar(tar, &src_str, &dest, &progress)?;
                    } else {
                        Self::extract_tar(reader, &src_str, &dest, &progress)?;
                    }
                }
                Ok(())
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("Join error: {}", e))),
        )
    }
}

impl Drop for ArchiveFs {
    fn drop(&mut self) {
        // tempfile::NamedTempFile will automatically delete the file when dropped.
        // We just need to ensure the Mutex is cleared if we are the last owner.
        // However, since it's an Arc<Mutex>, we can't easily clear it for other clones.
        // But NamedTempFile's drop is triggered when the *last* clone of the Arc<Mutex<Option<NamedTempFile>>> is dropped.
        // Actually, our tempfile is inside an Option inside a Mutex inside an Arc.
        // The file will be deleted when the NamedTempFile itself is dropped.
    }
}

impl ArchiveFs {
    fn read_tar_file<R: Read>(reader: R, path_str: &str) -> Result<Vec<u8>> {
        let mut archive = tar::Archive::new(reader);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let name = entry.path()?.to_string_lossy().replace('\\', "/");
            if name == path_str || name.trim_end_matches('/') == path_str {
                let mut buffer = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut buffer)?;
                return Ok(buffer);
            }
        }
        Err(anyhow::anyhow!("File not found in tar: {}", path_str))
    }

    fn extract_tar<R: Read>(
        reader: R,
        src_str: &str,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        let mut last_update = std::time::Instant::now();
        let mut p;
        let mut dir_mtimes = Vec::new();
        let mut archive = tar::Archive::new(reader);
        let is_root = src_str.is_empty() || src_str == ".";

        for entry in archive.entries()? {
            if progress.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let mut entry = entry?;
            let name_raw = entry.path()?.to_string_lossy().replace('\\', "/");
            let name = name_raw.trim_end_matches('/');

            let prefix = if is_root { "" } else { src_str };
            let should_extract = if is_root {
                true
            } else {
                name == src_str || name.starts_with(&format!("{}/", src_str))
            };

            if should_extract {
                let rel_path = if is_root {
                    PathBuf::from(name)
                } else {
                    Path::new(name)
                        .strip_prefix(prefix)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| PathBuf::from(name))
                };

                let rel_name_str = rel_path.to_string_lossy().to_string();

                if rel_name_str.is_empty() && entry.header().entry_type().is_dir() {
                    continue;
                }

                let target = if rel_name_str.is_empty() {
                    dest.to_path_buf()
                } else {
                    dest.join(&rel_name_str)
                };

                let mut mtime = None;
                if let Ok(mtime_secs) = entry.header().mtime() {
                    mtime =
                        Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(mtime_secs));
                }

                if entry.header().entry_type().is_dir() {
                    std::fs::create_dir_all(&target)?;
                    if let Some(mt) = mtime {
                        dir_mtimes.push((target, mt));
                    }
                } else {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut out = File::create(&target)?;
                    std::io::copy(&mut entry, &mut out)?;
                    let size = entry.size();
                    progress.processed_bytes.fetch_add(size, Ordering::Relaxed);

                    // Set modified time for file
                    if let Some(mt) = mtime {
                        let _ = set_file_mtime(&target, FileTime::from_system_time(mt));
                    }
                }

                p = progress.processed_items.fetch_add(1, Ordering::Relaxed) + 1;

                let now = std::time::Instant::now();
                if p % 10 == 0
                    || now.duration_since(last_update) > std::time::Duration::from_millis(100)
                {
                    let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateCurrentFile(
                        progress.id,
                        rel_name_str,
                    ));
                    let _ = progress
                        .tx
                        .send(crate::tasks::TaskEvent::UpdateByteProgress(
                            progress.id,
                            progress.processed_bytes.load(Ordering::Relaxed),
                            0,
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

        // Apply directory transitions in reverse order of depth to avoid spoiling
        dir_mtimes.sort_by(|a, b| b.0.as_os_str().len().cmp(&a.0.as_os_str().len()));
        for (dir, mtime) in dir_mtimes {
            let _ = set_file_mtime(&dir, FileTime::from_system_time(mtime));
        }

        // Send final update for this provider
        let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateProgress(
            progress.id,
            progress.processed_items.load(Ordering::Relaxed),
            0,
        ));
        Ok(())
    }

    fn system_tar_extract(
        archive_path: &Path,
        src_str: &str,
        dest: &Path,
        is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        use std::process::{Command, Stdio};

        let is_root = src_str.is_empty() || src_str == ".";

        // Ensure destination directory exists or parent if it's a file
        let working_dir = if is_dir || is_root {
            std::fs::create_dir_all(dest)?;
            dest.to_path_buf()
        } else if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
            parent.to_path_buf()
        } else {
            PathBuf::from(".")
        };

        let mut cmd = Command::new("tar");
        // -C must come before arguments that it should affect.
        // -x: extract, -v: verbose, -f: file.
        // Modern tar auto-detects compression.
        cmd.arg("-C").arg(&working_dir);
        cmd.arg("-xvf").arg(archive_path);

        if !is_root {
            // Ensure directories have a trailing slash for tar to match them correctly
            // and for --strip-components to work as expected.
            let mut tar_src = src_str.to_string();
            if is_dir && !tar_src.ends_with('/') {
                tar_src.push('/');
            }

            let path = Path::new(&tar_src);
            let count = path.components().count();
            // We want to strip components so that the extracted items land directly in dest
            let strip = if is_dir {
                count
            } else {
                count.saturating_sub(1)
            };
            if strip > 0 {
                cmd.arg(format!("--strip-components={}", strip));
            }
            cmd.arg(tar_src);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn().context("Failed to spawn tar")?;
        let stdout = child.stdout.take().context("Failed to open tar stdout")?;
        let reader = BufReader::new(stdout);

        let mut last_update = std::time::Instant::now();
        let mut p;

        for line in reader.lines() {
            if progress.cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                return Ok(());
            }
            if let Ok(file_path) = line {
                p = progress.processed_items.fetch_add(1, Ordering::Relaxed) + 1;
                let now = std::time::Instant::now();
                if now.duration_since(last_update) > std::time::Duration::from_millis(100) {
                    let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateCurrentFile(
                        progress.id,
                        file_path,
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

        let status = child.wait().context("Failed to wait for tar")?;
        if !status.success() {
            return Err(anyhow::anyhow!("Tar command failed with status {}", status));
        }

        // Handle rename if single file and names don't match
        if !is_root && !is_dir {
            let extracted_name = Path::new(src_str).file_name();
            let target_name = dest.file_name();
            if extracted_name != target_name
                && let (Some(en), Some(_tn)) = (extracted_name, target_name)
            {
                let extracted_path = working_dir.join(en);
                if extracted_path.exists() {
                    std::fs::rename(extracted_path, dest)?;
                }
            }
        }

        Ok(())
    }
}
