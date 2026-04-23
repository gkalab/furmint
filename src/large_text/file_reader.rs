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
