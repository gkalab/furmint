use anyhow::Result;
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};
use memmap2::Mmap;
use std::fs::File;
use std::path::PathBuf;

pub struct FileReader {
    mmap: Mmap,
    path: PathBuf,
    encoding: &'static Encoding,
}

impl FileReader {
    /// Creates a new `FileReader` by memory-mapping the file at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, metadata cannot be read,
    /// the file is empty, or memory-mapping fails.
    pub fn new(path: PathBuf, encoding: &'static Encoding) -> Result<Self> {
        let file = File::open(&path)?;
        let metadata = file.metadata()?;
        if metadata.len() == 0 {
            anyhow::bail!("Cannot memory-map an empty file: {}", path.display());
        }
        let mmap = unsafe { Mmap::map(&file)? };

        Ok(Self {
            mmap,
            path,
            encoding,
        })
    }

    #[must_use]
    pub fn get_chunk(&self, start: usize, end: usize) -> String {
        let end = end.min(self.mmap.len());
        if start >= end {
            return String::new();
        }

        let bytes = &self.mmap[start..end];
        let (cow, _encoding, _had_errors) = self.encoding.decode(bytes);
        cow.into_owned()
    }

    #[must_use]
    pub fn get_bytes(&self, start: usize, end: usize) -> &[u8] {
        let end = end.min(self.mmap.len());
        if start >= end {
            return &[];
        }
        &self.mmap[start..end]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }

    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    #[must_use]
    pub fn encoding(&self) -> &'static Encoding {
        self.encoding
    }

    #[must_use]
    pub fn all_data(&self) -> &[u8] {
        &self.mmap[..]
    }
}

#[must_use]
pub fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    // Check for BOM
    if bytes.len() >= 3 && bytes[0..3] == [0xEF, 0xBB, 0xBF] {
        return UTF_8;
    }
    if bytes.len() >= 2 {
        if bytes[0..2] == [0xFF, 0xFE] {
            return UTF_16LE;
        }
        if bytes[0..2] == [0xFE, 0xFF] {
            return UTF_16BE;
        }
    }

    // Try UTF-8 validation
    if std::str::from_utf8(bytes).is_ok() {
        return UTF_8;
    }

    // Default to WINDOWS_1252 (similar to ISO-8859-1)
    WINDOWS_1252
}

/// Detects if a chunk of bytes represents binary content.
///
/// This uses a heuristic similar to Git:
/// 1. If it has a UTF-16 BOM, it's considered text.
/// 2. If it contains a NULL byte, it's considered binary.
/// 3. If it contains more than 15% control characters, it's considered binary.
#[must_use]
pub fn is_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }

    // 1. Check for UTF-16/UTF-32 BOMs first (which contain null bytes)
    if bytes.len() >= 2 && (bytes[0..2] == [0xFF, 0xFE] || bytes[0..2] == [0xFE, 0xFF]) {
        return false;
    }
    if bytes.len() >= 4
        && (bytes[0..4] == [0x00, 0x00, 0xFE, 0xFF] || bytes[0..4] == [0xFF, 0xFE, 0x00, 0x00])
    {
        return false;
    }

    // 2. Check for NULL byte
    if bytes.contains(&0) {
        return true;
    }

    // 3. Check control characters ratio
    let mut control_chars = 0;
    for &b in bytes {
        if (b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t' && b != 0x0C) || b == 0x7F {
            control_chars += 1;
        }
    }

    // 15% threshold is common for binary detection
    control_chars * 100 > bytes.len() * 15
}
