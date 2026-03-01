use super::ArchiveFormat;
use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::traits::TaskProgressContext;
use crate::fs::utils::{FileEntry, mode_to_attributes};
use anyhow::{Context, Result};
use filetime::{FileTime, set_file_mtime};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::SystemTime;

pub struct TarHandler {
    path: PathBuf,
    temp_tar: Option<tempfile::NamedTempFile>,
}

impl TarHandler {
    pub fn new(path: &Path) -> Result<Self> {
        let mut handler = Self {
            path: path.to_path_buf(),
            temp_tar: None,
        };
        handler.decompress_if_needed()?;
        Ok(handler)
    }

    fn decompress_if_needed(&mut self) -> Result<()> {
        let ext = self
            .path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext == "tar" {
            return Ok(());
        }

        let mut temp = tempfile::NamedTempFile::new()?;
        let cmd_name = match ext.as_str() {
            "gz" | "tgz" => "gzip",
            "bz2" | "tbz2" => "bzip2",
            "xz" | "txz" => "xz",
            _ => return Ok(()),
        };

        let mut decompressed_via_system = false;
        if let Ok(mut child) = Command::new(cmd_name)
            .arg("-dc")
            .arg(&self.path)
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
            temp.as_file_mut().seek(std::io::SeekFrom::Start(0))?;
            temp.as_file_mut().set_len(0)?;
            let file = File::open(&self.path).context("Failed to open archive for fallback")?;
            let reader = BufReader::new(file);
            match ext.as_str() {
                "gz" | "tgz" => {
                    let mut decoder = flate2::read::GzDecoder::new(reader);
                    std::io::copy(&mut decoder, temp.as_file_mut())?;
                }
                "bz2" | "tbz2" => {
                    let mut decoder = bzip2::read::BzDecoder::new(reader);
                    std::io::copy(&mut decoder, temp.as_file_mut())?;
                }
                "xz" | "txz" => {
                    let mut decoder = xz2::read::XzDecoder::new(reader);
                    std::io::copy(&mut decoder, temp.as_file_mut())?;
                }
                _ => {}
            }
        }

        temp.as_file_mut().seek(std::io::SeekFrom::Start(0))?;
        self.temp_tar = Some(temp);
        Ok(())
    }

    fn get_reader(&self) -> Result<BufReader<File>> {
        let file = if let Some(ref temp) = self.temp_tar {
            File::open(temp.path())?
        } else {
            File::open(&self.path)?
        };
        Ok(BufReader::new(file))
    }
}

impl TarHandler {
    fn do_scan(&self) -> Result<super::ScanResult> {
        let reader = self.get_reader()?;
        let mut archive = tar::Archive::new(reader);
        let mut entries_map = HashMap::new();
        let mut tree_map: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();

        for file in archive.entries()? {
            Self::process_entry(file?, &mut entries_map, &mut tree_map)?;
        }

        let mut final_tree = HashMap::new();
        for (k, v) in tree_map {
            let mut children: Vec<PathBuf> = v.into_iter().collect();
            children.sort();
            final_tree.insert(k, children);
        }

        Ok((entries_map, final_tree))
    }

    fn process_entry<R: Read>(
        file: tar::Entry<'_, R>,
        entries_map: &mut HashMap<PathBuf, ArchiveEntry>,
        tree_map: &mut HashMap<PathBuf, HashSet<PathBuf>>,
    ) -> Result<()> {
        let path_owned = file.path()?.into_owned();
        let path_str = path_owned.to_string_lossy().replace('\\', "/");
        let normalized_path_str = path_str.trim_end_matches('/');
        let path = PathBuf::from(normalized_path_str);

        let is_dir = file.header().entry_type().is_dir();
        let size = file.size();
        let modified = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(file.header().mtime().unwrap_or(0));
        let position = file.raw_header_position();
        let is_symlink = file.header().entry_type().is_symlink();
        let unix_mode = file
            .header()
            .mode()
            .unwrap_or(if is_dir { 0o755 } else { 0o644 });
        let attributes = mode_to_attributes(unix_mode, is_dir, is_symlink);

        let entry = ArchiveEntry {
            file_entry: FileEntry {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                is_dir,
                is_symlink,
                size: Some(size),
                modified: Some(modified),
                attributes,
                selected: false,
            },
            position: Some(position),
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
        Ok(())
    }
}

impl ArchiveFormat for TarHandler {
    fn scan(&self) -> Result<super::ScanResult> {
        self.do_scan()
    }

    fn read_file(&self, path_str: &str) -> Result<Vec<u8>> {
        let reader = self.get_reader()?;
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
        Err(anyhow::anyhow!("File not found in tar: {path_str}"))
    }

    fn extract(
        &self,
        src_str: &str,
        dest: &Path,
        is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        let effective_path = if let Some(ref temp) = self.temp_tar {
            temp.path()
        } else {
            &self.path
        };

        // Try system tar first
        if self
            .system_tar_extract(effective_path, src_str, dest, is_dir, progress)
            .is_ok()
        {
            return Ok(());
        }

        // Fallback to Rust implementation
        let reader = self.get_reader()?;
        let mut archive = tar::Archive::new(reader);
        let is_root = src_str.is_empty() || src_str == ".";
        let mut dir_mtimes = Vec::new();
        let mut last_update = std::time::Instant::now();

        for entry in archive.entries()? {
            if progress.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let mut entry = entry?;
            let name_raw = entry.path()?.to_string_lossy().replace('\\', "/");
            let name = name_raw.trim_end_matches('/');

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
                if rel_name_str.is_empty() && entry.header().entry_type().is_dir() {
                    continue;
                }

                let target = if rel_name_str.is_empty() {
                    dest.to_path_buf()
                } else {
                    dest.join(&rel_name_str)
                };
                let mtime = entry
                    .header()
                    .mtime()
                    .ok()
                    .map(|m| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(m));

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
                    if let Some(mt) = mtime {
                        let _ = set_file_mtime(&target, FileTime::from_system_time(mt));
                    }
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

impl TarHandler {
    fn system_tar_extract(
        &self,
        archive_path: &Path,
        src_str: &str,
        dest: &Path,
        is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        let is_root = src_str.is_empty() || src_str == ".";
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
        cmd.arg("-C").arg(&working_dir);
        cmd.arg("-xvf").arg(archive_path);

        if !is_root {
            let mut tar_src = src_str.to_string();
            if is_dir && !tar_src.ends_with('/') {
                tar_src.push('/');
            }
            let path = Path::new(&tar_src);
            let count = path.components().count();
            let strip = if is_dir {
                count
            } else {
                count.saturating_sub(1)
            };
            if strip > 0 {
                cmd.arg(format!("--strip-components={strip}"));
            }
            cmd.arg(tar_src);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().context("Failed to open tar stdout")?;
        let reader = BufReader::new(stdout);
        let mut last_update = std::time::Instant::now();

        for line in reader.lines() {
            if progress.cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                return Ok(());
            }
            if let Ok(file_path) = line {
                let p = progress.processed_items.fetch_add(1, Ordering::Relaxed) + 1;
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

        if !child.wait()?.success() {
            return Err(anyhow::anyhow!("Tar command failed"));
        }

        if !is_root && !is_dir {
            let en = Path::new(src_str).file_name();
            let tn = dest.file_name();
            if en != tn
                && let (Some(en), Some(_tn)) = (en, tn)
            {
                let ep = working_dir.join(en);
                if ep.exists() {
                    std::fs::rename(ep, dest)?;
                }
            }
        }

        let p = progress.processed_items.load(Ordering::Relaxed);
        let _ = progress
            .tx
            .send(crate::tasks::TaskEvent::UpdateProgress(progress.id, p, 0));

        Ok(())
    }
}
