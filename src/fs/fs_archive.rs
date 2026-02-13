use crate::fs::fs_provider::FileSystemProvider;
use crate::fs::utils::FileEntry;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
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
}

impl ArchiveFs {
    pub fn new(path: &Path) -> Result<Self> {
        let entries = Arc::new(Mutex::new(HashMap::new()));
        let tree = Arc::new(Mutex::new(HashMap::new()));
        let fs = Self {
            archive_path: path.to_path_buf(),
            entries,
            tree,
        };

        fs.scan_archive()?;
        Ok(fs)
    }

    fn scan_archive(&self) -> Result<()> {
        let file = File::open(&self.archive_path).context("Failed to open archive")?;
        let reader = std::io::BufReader::new(file);

        let ext = self
            .archive_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "zip" {
            self.scan_zip(reader)?;
        } else if ext == "tar" {
            self.scan_tar(reader)?;
        } else if ext == "gz" || ext == "tgz" {
            // Use system gzip for better performance
            use std::process::{Command, Stdio};

            let child = Command::new("gzip")
                .arg("-dc")
                .arg(&self.archive_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();

            match child {
                Ok(mut child) => {
                    if let Some(stdout) = child.stdout.take() {
                        let reader = std::io::BufReader::new(stdout);
                        let res = self.scan_tar(reader);
                        let _ = child.wait(); // extensive wait might not be strictly necessary if we drop stdout, but good practice
                        res?;
                    } else {
                        return Err(anyhow::anyhow!("Failed to open stdout of gzip process"));
                    }
                }
                Err(_e) => {
                    let file = File::open(&self.archive_path)
                        .context("Failed to open archive for fallback")?;
                    let reader = std::io::BufReader::new(file);
                    let tar = flate2::read::GzDecoder::new(reader);
                    self.scan_tar(tar)?;
                }
            }
        } else if ext == "bz2" || ext == "tbz2" {
            // Assume tar.bz2
            let tar = bzip2::read::BzDecoder::new(reader);
            self.scan_tar(tar)?;
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

            // Normalize path separators
            let path_str = name.replace('\\', "/");
            let path = PathBuf::from(&path_str);

            let is_dir = file.is_dir() || name.ends_with('/');

            let size = file.size();
            // Simple conversion of ZipDateTime to SystemTime (approximate)
            let _dt = file.last_modified();
            let modified = SystemTime::UNIX_EPOCH; // Placeholder

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
            let path = file.path()?.into_owned();
            // Normalize path separators
            let path_str = path.to_string_lossy().replace('\\', "/");
            let path = PathBuf::from(&path_str);

            let is_dir = file.header().entry_type().is_dir();
            let size = file.size();
            let modified = SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(file.header().mtime().unwrap_or(0));

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

    fn read_file(&self, _path: &Path) -> Result<Vec<u8>> {
        Err(anyhow::anyhow!(
            "Reading files from archive not yet supported"
        ))
    }

    fn read_file_at(&self, _path: &Path, _offset: u64, _len: usize) -> Result<Vec<u8>> {
        Err(anyhow::anyhow!(
            "Reading files from archive not yet supported"
        ))
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
        let p = if rel_path == Path::new("") {
            Path::new(".")
        } else {
            rel_path
        };
        self.entries.lock().unwrap().contains_key(p) || p == Path::new(".")
    }

    fn is_dir(&self, path: &Path) -> bool {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p = if rel_path == Path::new("") {
            Path::new(".")
        } else {
            rel_path
        };

        if p == Path::new(".") {
            return true;
        }

        self.entries
            .lock()
            .unwrap()
            .get(p)
            .map(|e| e.is_dir)
            .unwrap_or(false)
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
        let p = if rel_path == Path::new("") {
            Path::new(".")
        } else {
            rel_path
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
}
