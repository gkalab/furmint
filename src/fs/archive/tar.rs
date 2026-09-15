use super::ArchiveFormat;
use super::common::{self, ArchiveEntryMetadata};
use crate::fs::fs_provider::TaskProgressContext;
use crate::fs::utils::mode_to_attributes;
use anyhow::{Context, Result};
use std::collections::HashMap;
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
    /// Creates a new `TarHandler`, decompressing the archive if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be decompressed.
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
            // Cap the decompressed size (zip-bomb guard).
            && common::copy_bounded(
                &mut stdout,
                temp.as_file_mut(),
                common::MAX_TOTAL_EXTRACT_BYTES,
                "decompressed archive",
            )
            .is_ok()
            && child.wait().is_ok_and(|s| s.success())
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
                    common::copy_bounded(
                        &mut decoder,
                        temp.as_file_mut(),
                        common::MAX_TOTAL_EXTRACT_BYTES,
                        "decompressed archive",
                    )?;
                }
                "bz2" | "tbz2" => {
                    let mut decoder = bzip2::read::BzDecoder::new(reader);
                    common::copy_bounded(
                        &mut decoder,
                        temp.as_file_mut(),
                        common::MAX_TOTAL_EXTRACT_BYTES,
                        "decompressed archive",
                    )?;
                }
                "xz" | "txz" => {
                    let mut decoder = xz2::read::XzDecoder::new(reader);
                    common::copy_bounded(
                        &mut decoder,
                        temp.as_file_mut(),
                        common::MAX_TOTAL_EXTRACT_BYTES,
                        "decompressed archive",
                    )?;
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
        let mut tree_map = HashMap::new();

        for entry in archive.entries()? {
            let entry = entry?;
            let path_owned = entry.path()?.into_owned();
            let path = common::normalize_path(&path_owned.to_string_lossy());

            let is_dir = entry.header().entry_type().is_dir();
            let size = entry.size();
            let modified = SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(entry.header().mtime().unwrap_or(0));
            let position = entry.raw_header_position();
            let is_symlink = entry.header().entry_type().is_symlink();
            let unix_mode = entry
                .header()
                .mode()
                .unwrap_or(if is_dir { 0o755 } else { 0o644 });
            let attributes = mode_to_attributes(unix_mode, is_dir, is_symlink);

            common::add_to_tree(
                path,
                ArchiveEntryMetadata {
                    is_dir,
                    is_symlink,
                    size: Some(size),
                    modified: Some(modified),
                    attributes,
                    position: Some(position),
                },
                &mut entries_map,
                &mut tree_map,
            );
        }

        Ok((entries_map, common::finalize_tree(tree_map)))
    }
}

impl ArchiveFormat for TarHandler {
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
        let reader = self.get_reader()?;
        let mut archive = tar::Archive::new(reader);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let name = entry.path()?.to_string_lossy().replace('\\', "/");
            if name == path_str || name.trim_end_matches('/') == path_str {
                // Grow the buffer naturally instead of pre-reserving the
                // header's claimed size, which is untrusted (a hostile
                // archive could claim gigabytes).
                let mut buffer = Vec::new();
                entry.read_to_end(&mut buffer)?;
                return Ok(buffer);
            }
        }
        Err(anyhow::anyhow!("File not found in tar: {path_str}"))
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
        let effective_path = if let Some(ref temp) = self.temp_tar {
            temp.path()
        } else {
            &self.path
        };
        let dest = common::canonicalize_dest(dest);

        // Try system tar first
        if Self::system_tar_extract(effective_path, src_str, &dest, is_dir, progress).is_ok() {
            return Ok(());
        }

        // Fallback to Rust implementation
        let reader = self.get_reader()?;
        let mut archive = tar::Archive::new(reader);
        let mut dir_mtimes = Vec::new();
        let mut last_update = std::time::Instant::now();
        let opts = common::ExtractOptions {
            src_str,
            dest: &dest,
            is_dir,
            progress,
        };

        for entry in archive.entries()? {
            if progress.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let mut entry = entry?;
            let name_raw = entry.path()?.to_string_lossy().to_string();
            let is_entry_dir = entry.header().entry_type().is_dir();
            let is_symlink = entry.header().entry_type().is_symlink();
            let size = entry.size();
            let mtime = entry
                .header()
                .mtime()
                .ok()
                .map(|m| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(m));

            let mode = entry.header().mode().ok();
            common::handle_extraction_entry(
                &mut entry,
                &common::ExtractionEntryMetadata {
                    name_raw: &name_raw,
                    is_dir: is_entry_dir,
                    is_symlink,
                    size,
                    mtime,
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
}

impl TarHandler {
    fn system_tar_extract(
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
                    let _ = progress.tx.send(crate::tasks::UiEvent::Task(
                        crate::tasks::TaskEvent::UpdateCurrentFile {
                            task_id: progress.id,
                            filename: file_path,
                        },
                    ));
                    let _ = progress.tx.send(crate::tasks::UiEvent::Task(
                        crate::tasks::TaskEvent::UpdateProgress {
                            task_id: progress.id,
                            processed: p,
                            total: 0,
                        },
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
        let _ = progress.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateProgress {
                task_id: progress.id,
                processed: p,
                total: 0,
            },
        ));

        Ok(())
    }
}
