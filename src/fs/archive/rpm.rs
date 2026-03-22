use crate::fs::archive::{ArchiveFormat, ScanResult, common};
use crate::fs::fs_archive::ArchiveEntry;
use crate::fs::traits::TaskProgressContext;
use crate::fs::utils::FileEntry;
use anyhow::{Result, anyhow};
use cpio::NewcReader;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

pub struct RpmHandler {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum RpmValue {
    String(String),
    Int32(i32),
    Int64(i64),
    StringArray(Vec<String>),
    Binary(Vec<u8>),
}

impl RpmValue {
    #[must_use]
    pub fn as_string(&self) -> String {
        match self {
            RpmValue::String(s) => s.clone(),
            RpmValue::Int32(i) => i.to_string(),
            RpmValue::Int64(i) => i.to_string(),
            RpmValue::StringArray(arr) => arr.join(", "),
            RpmValue::Binary(_) => "<binary>".to_string(),
        }
    }
}

impl RpmHandler {
    #[must_use]
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn skip_header_structure<R: Read + Seek>(reader: &mut R, padded: bool) -> Result<()> {
        Self::parse_header_internal(reader)?;

        if padded {
            // Padding to 8-byte boundary
            let pos = reader.stream_position()?;
            let pad = (8 - (pos % 8)) % 8;
            if pad > 0 {
                let pad = i64::try_from(pad).unwrap_or(0);
                reader.seek(SeekFrom::Current(pad))?;
            }
        }

        Ok(())
    }

    fn parse_header_internal<R: Read + Seek>(reader: &mut R) -> Result<HashMap<i32, RpmValue>> {
        let mut magic = [0u8; 3];
        reader.read_exact(&mut magic)?;
        if &magic != b"\x8e\xad\xe8" {
            return Err(anyhow!("Invalid RPM header magic"));
        }

        let mut version = [0u8; 1];
        reader.read_exact(&mut version)?;
        if version[0] != 1 {
            return Err(anyhow!("Unsupported RPM header version: {}", version[0]));
        }

        let mut reserved = [0u8; 4];
        reader.read_exact(&mut reserved)?;

        let mut count_buf = [0u8; 4];
        reader.read_exact(&mut count_buf)?;
        let count = u32::from_be_bytes(count_buf);

        let mut size_buf = [0u8; 4];
        reader.read_exact(&mut size_buf)?;
        let size = u32::from_be_bytes(size_buf);

        let mut index_entries = Vec::new();
        for _ in 0..count {
            let mut buf = [0u8; 16];
            reader.read_exact(&mut buf)?;
            index_entries.push((
                i32::from_be_bytes(buf[0..4].try_into().unwrap()),
                i32::from_be_bytes(buf[4..8].try_into().unwrap()),
                i32::from_be_bytes(buf[8..12].try_into().unwrap()),
                i32::from_be_bytes(buf[12..16].try_into().unwrap()),
            ));
        }

        let mut data = vec![0u8; size as usize];
        reader.read_exact(&mut data)?;

        let mut tags = HashMap::new();
        for (tag, ty, offset, cnt) in index_entries {
            let offset = usize::try_from(offset).unwrap_or(0);
            let cnt = u32::try_from(cnt).unwrap_or(0);
            let val = match ty {
                6 | 9 => {
                    // STRING, I18NSTRING
                    let s = data[offset..].split(|&b| b == 0).next().unwrap_or(&[]);
                    RpmValue::String(String::from_utf8_lossy(s).to_string())
                }
                4 => {
                    // INT32
                    match cnt.cmp(&1) {
                        std::cmp::Ordering::Equal => {
                            let i =
                                i32::from_be_bytes(data[offset..offset + 4].try_into().unwrap());
                            RpmValue::Int32(i)
                        }
                        std::cmp::Ordering::Greater => {
                            let mut arr = Vec::new();
                            for i in 0..cnt {
                                let start = offset + (i as usize) * 4;
                                arr.push(i32::from_be_bytes(
                                    data[start..start + 4].try_into().unwrap(),
                                ));
                            }
                            RpmValue::String(
                                arr.iter()
                                    .map(std::string::ToString::to_string)
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            )
                        }
                        std::cmp::Ordering::Less => RpmValue::Int32(0),
                    }
                }
                8 => {
                    // STRING_ARRAY
                    let mut arr = Vec::new();
                    let mut start = offset;
                    for _ in 0..cnt {
                        let s = data[start..].split(|&b| b == 0).next().unwrap_or(&[]);
                        arr.push(String::from_utf8_lossy(s).to_string());
                        start += s.len() + 1;
                    }
                    RpmValue::StringArray(arr)
                }
                _ => RpmValue::Binary(vec![]), // Placeholder for other types
            };
            tags.insert(tag, val);
        }

        Ok(tags)
    }

    /// Extracts metadata from the RPM package.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPM file cannot be read or parsed.
    pub fn get_metadata(&self) -> Result<String> {
        let mut rpm_file = File::open(&self.path)?;

        // Skip Lead (96 bytes)
        rpm_file.seek(SeekFrom::Start(96))?;

        // Parse Signature Header (padded to 8 bytes)
        let sig_tags = Self::parse_header_internal(&mut rpm_file)?;
        // Padding to 8-byte boundary after signature header
        let pos = rpm_file.stream_position()?;
        let pad = (8 - (pos % 8)) % 8;
        if pad > 0 {
            let pad = i64::try_from(pad).unwrap_or(0);
            rpm_file.seek(SeekFrom::Current(pad))?;
        }

        // Parse Main Header
        let tags = Self::parse_header_internal(&mut rpm_file)?;

        let mut result = String::new();

        // Check for signatures in sig_tags
        // 1000: SIGTAG_PGP, 1005: SIGTAG_GPG, 268: SIGTAG_RSA, 267: SIGTAG_DSA
        let is_signed = sig_tags.contains_key(&1000)
            || sig_tags.contains_key(&1005)
            || sig_tags.contains_key(&268)
            || sig_tags.contains_key(&267);

        let common_tags = [
            (1000, "Name"),
            (1001, "Version"),
            (1002, "Release"),
            (1022, "Architecture"),
            (1016, "Summary"),
            (1011, "License"),
            (1014, "Packager"),
            (1020, "URL"),
            (1021, "OS"),
            (1006, "Build Date"),
            (1007, "Build Host"),
            (1023, "Vendor"),
            (1009, "Size"),
            (-1, "Signed"),
            (1005, "Description"),
        ];

        for (tag, label) in common_tags {
            if tag == -1 {
                // Special case for Signed
                use std::fmt::Write;
                let _ = writeln!(
                    result,
                    "{:<15}: {}",
                    label,
                    if is_signed { "yes" } else { "no" }
                );
                continue;
            }

            if let Some(val) = tags.get(&tag) {
                if tag == 1006 {
                    // Build Date - format timestamp
                    if let RpmValue::Int32(t) = val {
                        use chrono::TimeZone;
                        let dt = chrono::Local.timestamp_opt(i64::from(*t), 0).single();
                        if let Some(d) = dt {
                            use std::fmt::Write;
                            let _ = writeln!(
                                result,
                                "{:<15}: {}",
                                label,
                                d.format("%Y-%m-%d %H:%M:%S")
                            );
                            continue;
                        }
                    }
                } else if tag == 1009 {
                    // Size - format human readable
                    if let RpmValue::Int32(s) = val {
                        use std::fmt::Write;
                        let _ = writeln!(
                            result,
                            "{:<15}: {}",
                            label,
                            crate::fs::utils::format_size(
                                Some(u64::from(u32::try_from(*s).unwrap_or(0))),
                                false,
                                false
                            )
                            .trim_start()
                        );
                        continue;
                    }
                }
                let _ = {
                    use std::fmt::Write;
                    writeln!(result, "{:<15}: {}", label, val.as_string())
                };
            }
        }

        Ok(result)
    }

    fn get_payload_reader(&self) -> Result<Box<dyn Read>> {
        let mut rpm_file = File::open(&self.path)?;

        // Skip Lead (96 bytes)
        rpm_file.seek(SeekFrom::Start(96))?;

        // Skip Signature Header Structure (padded to 8 bytes)
        Self::skip_header_structure(&mut rpm_file, true)?;

        // Parse Main Header
        let _ = Self::parse_header_internal(&mut rpm_file)?;

        // Now we are at the payload. It might be compressed.
        // We need to guess the compression.
        let mut magic = [0u8; 6];
        rpm_file.read_exact(&mut magic)?;
        rpm_file.seek(SeekFrom::Current(-6))?;

        let inner: Box<dyn Read> = if &magic[0..2] == b"\x1f\x8b" {
            // Gzip
            Box::new(flate2::read::GzDecoder::new(rpm_file))
        } else if &magic[0..3] == b"BZh" {
            // Bzip2
            Box::new(bzip2::read::BzDecoder::new(rpm_file))
        } else if &magic[0..6] == b"\xfd7zXZ\x00" {
            // XZ
            Box::new(xz2::read::XzDecoder::new(rpm_file))
        } else if &magic[0..4] == b"\x28\xb5\x2f\xfd" {
            // Zstd
            Box::new(zstd::stream::read::Decoder::new(rpm_file)?)
        } else {
            // Assume uncompressed or unknown
            Box::new(rpm_file)
        };

        Ok(inner)
    }
}

impl ArchiveFormat for RpmHandler {
    fn scan(&self) -> Result<ScanResult> {
        let mut entries = HashMap::new();
        let mut tree: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

        let mut reader = self.get_payload_reader()?;
        loop {
            let Ok(entry_reader) = NewcReader::new(reader) else {
                break;
            };

            let name = entry_reader.entry().name().to_string();
            if name == "TRAILER!!!" {
                break;
            }

            let p = PathBuf::from(name.trim_start_matches('.').trim_start_matches('/'));
            if p == Path::new("") {
                reader = entry_reader.finish()?;
                continue;
            }

            let is_dir = entry_reader.entry().mode() & 0o040_000 != 0;
            let size = if is_dir {
                None
            } else {
                Some(u64::from(entry_reader.entry().file_size()))
            };

            let modified = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(u64::from(entry_reader.entry().mtime()));

            let file_entry = FileEntry {
                name: p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                is_dir,
                is_symlink: entry_reader.entry().mode() & 0o120_000 == 0o120_000,
                size,
                modified: Some(modified),
                attributes: crate::fs::utils::mode_to_attributes(
                    entry_reader.entry().mode(),
                    is_dir,
                    entry_reader.entry().mode() & 0o120_000 == 0o120_000,
                ),
                selected: false,
            };

            entries.insert(
                p.clone(),
                ArchiveEntry {
                    file_entry,
                    position: None,
                },
            );

            let mut curr = p.clone();
            while let Some(parent) = curr.parent() {
                let parent_path = if parent == Path::new("") {
                    PathBuf::from(".")
                } else {
                    parent.to_path_buf()
                };

                // Ensure parent exists in entries
                if parent != Path::new("") && !entries.contains_key(&parent_path) {
                    let dir_entry = FileEntry {
                        name: parent_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        is_dir: true,
                        is_symlink: false,
                        size: None,
                        modified: None,
                        attributes: crate::fs::utils::mode_to_attributes(0o40755, true, false),
                        selected: false,
                    };
                    entries.insert(
                        parent_path.clone(),
                        ArchiveEntry {
                            file_entry: dir_entry,
                            position: None,
                        },
                    );
                }

                let children = tree.entry(parent_path.clone()).or_default();
                if !children.contains(&curr) {
                    children.push(curr.clone());
                }
                if parent == Path::new("") {
                    break;
                }
                curr = parent_path;
            }

            reader = entry_reader.finish()?;
        }

        Ok((entries, tree))
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let mut reader = self.get_payload_reader()?;
        loop {
            let Ok(mut entry_reader) = NewcReader::new(reader) else {
                break;
            };

            let name = entry_reader.entry().name().to_string();
            if name == "TRAILER!!!" {
                break;
            }

            let p = PathBuf::from(name.trim_start_matches('.').trim_start_matches('/'));
            let p_str = p.to_string_lossy().replace('\\', "/");
            let p_str = p_str.trim_end_matches('/');

            if p_str == path {
                let mut data = Vec::new();
                entry_reader.read_to_end(&mut data)?;
                return Ok(data);
            }
            reader = entry_reader.finish()?;
        }

        Err(anyhow!("File not found in RPM: {path}"))
    }

    fn extract(
        &self,
        src_path: &str,
        dest: &Path,
        is_dir: bool,
        progress: &TaskProgressContext,
    ) -> Result<()> {
        let mut reader = self.get_payload_reader()?;

        let opts = common::ExtractOptions {
            src_str: src_path,
            dest,
            is_dir,
            progress,
        };

        let mut dir_mtimes = Vec::new();
        let mut last_update = std::time::Instant::now();

        loop {
            let Ok(mut entry_reader) = NewcReader::new(reader) else {
                break;
            };

            let name_raw = entry_reader.entry().name().to_string();
            if name_raw == "TRAILER!!!" {
                break;
            }

            let name = name_raw.trim_start_matches('.').trim_start_matches('/');
            let is_entry_dir = entry_reader.entry().mode() & 0o040_000 != 0;
            let is_symlink = entry_reader.entry().mode() & 0o120_000 == 0o120_000;
            let size = u64::from(entry_reader.entry().file_size());
            let mtime = Some(
                std::time::SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs(u64::from(entry_reader.entry().mtime())),
            );

            let mode = Some(entry_reader.entry().mode());
            common::handle_extraction_entry(
                &mut entry_reader,
                &common::ExtractionEntryMetadata {
                    name_raw: name,
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

            reader = entry_reader.finish()?;
        }

        common::preserve_mtimes(dir_mtimes);
        let p_final = progress.processed_items.load(Ordering::Relaxed);
        let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateProgress(
            progress.id,
            p_final,
            0,
        ));

        let p_final = progress.processed_items.load(Ordering::Relaxed);
        let _ = progress.tx.send(crate::tasks::TaskEvent::UpdateProgress(
            progress.id,
            p_final,
            0,
        ));

        Ok(())
    }
}
