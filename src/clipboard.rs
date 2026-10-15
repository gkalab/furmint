use crate::fs::provider::FileSystemProvider;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClipboardAction {
    Copy,
    Cut,
}

#[derive(Clone)]
pub struct FileClipboardData {
    pub action: FileClipboardAction,
    pub paths: Vec<PathBuf>,
    pub source_provider: Arc<dyn FileSystemProvider>,
}

pub trait FileClipboard: Send {
    /// Sets the clipboard data.
    ///
    /// # Errors
    ///
    /// Returns an error if the clipboard cannot be updated.
    fn set(&mut self, data: FileClipboardData) -> anyhow::Result<()>;
    /// Gets the clipboard data.
    ///
    /// # Errors
    ///
    /// Returns an error if the clipboard cannot be read.
    fn get(&mut self) -> anyhow::Result<Option<FileClipboardData>>;
    /// Clears the clipboard.
    ///
    /// # Errors
    ///
    /// Returns an error if the clipboard cannot be cleared.
    fn clear(&mut self) -> anyhow::Result<()>;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix_clipboard {
    use super::{FileClipboard, FileClipboardData};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct UnixFileClipboard {
        inner: Arc<Mutex<Option<FileClipboardData>>>,
    }

    impl UnixFileClipboard {
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl FileClipboard for UnixFileClipboard {
        fn set(&mut self, data: FileClipboardData) -> anyhow::Result<()> {
            *self.inner.lock().unwrap() = Some(data);
            Ok(())
        }

        fn get(&mut self) -> anyhow::Result<Option<FileClipboardData>> {
            Ok(self.inner.lock().unwrap().clone())
        }

        fn clear(&mut self) -> anyhow::Result<()> {
            *self.inner.lock().unwrap() = None;
            Ok(())
        }
    }
}

#[cfg(target_os = "windows")]
pub mod win_clipboard;

/// In-memory clipboard for tests - does not use OS clipboard
/// Use this in tests to avoid parallel test interference on Windows
#[cfg(test)]
#[derive(Clone, Default)]
pub struct InMemoryFileClipboard {
    inner: Option<FileClipboardData>,
}

#[cfg(test)]
impl InMemoryFileClipboard {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
impl FileClipboard for InMemoryFileClipboard {
    fn set(&mut self, data: FileClipboardData) -> anyhow::Result<()> {
        self.inner = Some(data);
        Ok(())
    }

    fn get(&mut self) -> anyhow::Result<Option<FileClipboardData>> {
        Ok(self.inner.clone())
    }

    fn clear(&mut self) -> anyhow::Result<()> {
        self.inner = None;
        Ok(())
    }
}

pub enum ClipboardBackend {
    #[cfg(target_os = "windows")]
    Windows(win_clipboard::WindowsFileClipboard),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Unix(unix_clipboard::UnixFileClipboard),
}

impl ClipboardBackend {
    #[must_use]
    pub fn new() -> Self {
        #[cfg(target_os = "windows")]
        {
            ClipboardBackend::Windows(win_clipboard::WindowsFileClipboard::new())
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            ClipboardBackend::Unix(unix_clipboard::UnixFileClipboard::new())
        }
    }
}

impl Default for ClipboardBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl FileClipboard for ClipboardBackend {
    fn set(&mut self, data: FileClipboardData) -> anyhow::Result<()> {
        match self {
            #[cfg(target_os = "windows")]
            ClipboardBackend::Windows(c) => c.set(data),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            ClipboardBackend::Unix(c) => c.set(data),
        }
    }

    fn get(&mut self) -> anyhow::Result<Option<FileClipboardData>> {
        match self {
            #[cfg(target_os = "windows")]
            ClipboardBackend::Windows(c) => c.get(),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            ClipboardBackend::Unix(c) => c.get(),
        }
    }

    fn clear(&mut self) -> anyhow::Result<()> {
        match self {
            #[cfg(target_os = "windows")]
            ClipboardBackend::Windows(c) => c.clear(),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            ClipboardBackend::Unix(c) => c.clear(),
        }
    }
}

#[cfg(test)]
mod tests;
