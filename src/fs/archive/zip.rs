use super::ArchiveFormat;
use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::traits::TaskProgressContext;
use crate::fs::utils::{FileEntry, mode_to_attributes};
use anyhow::{Context, Result};
use chrono::TimeZone;
use filetime::{FileTime, set_file_mtime};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::SystemTime;

pub struct ZipHandler {
    path: PathBuf,
}

impl ZipHandler {
    pub fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
        })
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

impl ArchiveFormat for ZipHandler {
    fn scan(&self) -> Result<super::ScanResult> {
        let file = File::open(&self.path).context("Failed to open zip archive")?;
        let reader = std::io::BufReader::new(file);
        let mut archive = zip::ZipArchive::new(reader).context("Failed to read zip archive")?;

        let mut entries_map = HashMap::new();
        let mut tree_map: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let name = file.name().to_string();

            let path_str = name.replace('\\', "/");
            let normalized_path_str = path_str.trim_end_matches('/');
            let path = PathBuf::from(normalized_path_str);

            let is_dir = file.is_dir() || name.ends_with('/');
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

            let entry = ArchiveEntry {
                file_entry: FileEntry {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    is_dir,
                    is_symlink: false,
                    size: Some(size),
                    modified: Some(modified),
                    attributes,
                    selected: false,
                },
                position: None,
            };

            entries_map.insert(path.clone(), entry);

            if let Some(parent) = path.parent() {
                let parent = if parent == Path::new("") {
                    Path::new(".").to_path_buf()
                } else {
                    parent.to_path_buf()
                };
                tree_map.entry(parent).or_default().insert(path.clone());

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
                        let implicit_entry = ArchiveEntry {
                            file_entry: FileEntry {
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
                            },
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

        let mut final_tree = HashMap::new();
        for (k, v) in tree_map {
            let mut children: Vec<PathBuf> = v.into_iter().collect();
            children.sort();
            final_tree.insert(k, children);
        }

        Ok((entries_map, final_tree))
    }

    fn read_file(&self, path_str: &str) -> Result<Vec<u8>> {
        let file = File::open(&self.path).context("Failed to open archive")?;
        let mut archive = zip::ZipArchive::new(file).context("Failed to read zip")?;
        let mut zip_file = archive.by_name(path_str).context("File not found in zip")?;
        let mut buffer = Vec::with_capacity(zip_file.size() as usize);
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
                name == src_str || name.starts_with(&format!("{}/", src_str))
            };

            if should_extract {
                let rel_path = if is_root {
                    PathBuf::from(name)
                } else {
                    Path::new(name)
                        .strip_prefix(src_str)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| PathBuf::from(name))
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

        dir_mtimes.sort_by(|a, b| b.0.as_os_str().len().cmp(&a.0.as_os_str().len()));
        for (dir, mtime) in dir_mtimes {
            let _ = set_file_mtime(&dir, FileTime::from_system_time(mtime));
        }

        let p = progress.processed_items.load(Ordering::Relaxed);
        let _ = progress
            .tx
            .send(crate::tasks::TaskEvent::UpdateProgress(progress.id, p, 0));

        Ok(())
    }
}
