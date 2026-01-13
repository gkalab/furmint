use crate::fs_provider::FileSystemProvider;
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
    fn set(&mut self, data: FileClipboardData) -> anyhow::Result<()>;
    fn get(&mut self) -> anyhow::Result<Option<FileClipboardData>>;
    fn clear(&mut self) -> anyhow::Result<()>;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix_clipboard {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct UnixFileClipboard {
        inner: Arc<Mutex<Option<FileClipboardData>>>,
    }

    impl UnixFileClipboard {
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
pub mod win_clipboard {
    use super::*;
    use anyhow::Context;
    use std::ffi::OsStr;

    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
        SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows::core::w;

    const CF_HDROP: u32 = 15;

    const DROPEFFECT_COPY: u32 = 1;
    const DROPEFFECT_MOVE: u32 = 2;

    pub struct WindowsFileClipboard {
        cache: Arc<Mutex<Option<FileClipboardData>>>,
    }

    impl WindowsFileClipboard {
        pub fn new() -> Self {
            Self {
                cache: Arc::new(Mutex::new(None)),
            }
        }
    }

    fn to_drop_effect(action: FileClipboardAction) -> u32 {
        match action {
            FileClipboardAction::Copy => DROPEFFECT_COPY,
            FileClipboardAction::Cut => DROPEFFECT_MOVE,
        }
    }

    fn from_drop_effect(effect: u32) -> Option<FileClipboardAction> {
        if effect & DROPEFFECT_MOVE != 0 {
            Some(FileClipboardAction::Cut)
        } else if effect & DROPEFFECT_COPY != 0 {
            Some(FileClipboardAction::Copy)
        } else {
            None
        }
    }

    fn paths_to_dropfiles_buffer(paths: &[PathBuf]) -> Vec<u8> {
        let mut wide: Vec<u16> = Vec::new();
        for p in paths {
            let s: &OsStr = p.as_os_str();
            let mut v: Vec<u16> = s.encode_wide().collect();
            v.push(0);
            wide.extend_from_slice(&v);
        }
        wide.push(0);

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Dropfiles {
            p_files: u32,
            pt_x: i32,
            pt_y: i32,
            f_nc: i32,   // Replacing BOOL (4 bytes) with i32
            f_wide: i32, // Replacing BOOL (4 bytes) with i32
        }

        let header = Dropfiles {
            p_files: std::mem::size_of::<Dropfiles>() as u32,
            pt_x: 0,
            pt_y: 0,
            f_nc: 0,
            f_wide: 1,
        };

        let mut buf = Vec::with_capacity(std::mem::size_of::<Dropfiles>() + wide.len() * 2);

        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                &header as *const Dropfiles as *const u8,
                std::mem::size_of::<Dropfiles>(),
            )
        };
        buf.extend_from_slice(header_bytes);

        for w in wide {
            buf.extend_from_slice(&w.to_le_bytes());
        }

        buf
    }

    unsafe fn alloc_global_from_bytes(bytes: &[u8]) -> anyhow::Result<isize> {
        let hglobal = unsafe {
            GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len())
                .map_err(|e| anyhow::anyhow!("GlobalAlloc failed: {}", e))?
        };

        if hglobal.0.is_null() {
            anyhow::bail!("GlobalAlloc failed");
        }

        let ptr = unsafe { GlobalLock(hglobal) };
        if ptr.is_null() {
            unsafe {
                let _ = GlobalFree(Some(hglobal));
            }
            anyhow::bail!("GlobalLock failed");
        }

        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
            let _ = GlobalUnlock(hglobal);
        }

        Ok(hglobal.0 as isize)
    }

    unsafe fn read_u32_from_hglobal(hglobal: isize) -> Option<u32> {
        if hglobal == 0 {
            return None;
        }
        let h = HGLOBAL(hglobal as *mut _);
        let ptr = unsafe { GlobalLock(h) };
        if ptr.is_null() {
            return None;
        }
        let value = unsafe { *(ptr as *const u32) };
        unsafe {
            let _ = GlobalUnlock(h);
        }
        Some(value)
    }

    struct ClipboardGuard;

    impl ClipboardGuard {
        fn open() -> anyhow::Result<Self> {
            unsafe {
                let mut attempts = 0;
                loop {
                    if OpenClipboard(None).is_ok() {
                        return Ok(Self);
                    }
                    attempts += 1;
                    if attempts >= 20 {
                        break;
                    }
                    // Increase sleep duration slightly each time
                    std::thread::sleep(std::time::Duration::from_millis(attempts * 5));
                }
                OpenClipboard(None).context("OpenClipboard failed after 20 retries")?;
                Ok(Self)
            }
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    impl FileClipboard for WindowsFileClipboard {
        fn set(&mut self, data: FileClipboardData) -> anyhow::Result<()> {
            // Update in-process cache
            *self.cache.lock().unwrap() = Some(data.clone());

            let _guard = ClipboardGuard::open()?;
            unsafe {
                EmptyClipboard().context("EmptyClipboard failed")?;

                let buf = paths_to_dropfiles_buffer(&data.paths);
                let hglobal = alloc_global_from_bytes(&buf)?;
                SetClipboardData(CF_HDROP, Some(HANDLE(hglobal as *mut _)))
                    .context("SetClipboardData CF_HDROP failed")?;

                let format = RegisterClipboardFormatW(w!("Preferred DropEffect"));
                if format != 0 {
                    let _effect = to_drop_effect(data.action);
                    let hglobal_effect =
                        GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, std::mem::size_of::<u32>())
                            .map_err(|e| {
                                anyhow::anyhow!("GlobalAlloc for DropEffect failed: {}", e)
                            })?;

                    if !hglobal_effect.0.is_null() {
                        let ptr = GlobalLock(hglobal_effect);
                        if !ptr.is_null() {
                            *(ptr as *mut u32) = to_drop_effect(data.action);
                            let _ = GlobalUnlock(hglobal_effect);
                            if let Err(e) = SetClipboardData(format, Some(HANDLE(hglobal_effect.0)))
                            {
                                // If this fails, it's not fatal, but we should log it or something
                                // and we definitely shouldn't leak hglobal_effect if it wasn't taken.
                                // Actually SetClipboardData documentation says if it fails, the caller owns the memory.
                                let _ = GlobalFree(Some(hglobal_effect));
                                return Err(anyhow::anyhow!(
                                    "SetClipboardData format failed: {}",
                                    e
                                ));
                            }
                        } else {
                            let _ = GlobalFree(Some(hglobal_effect));
                        }
                    }
                }
            }
            Ok(())
        }

        fn get(&mut self) -> anyhow::Result<Option<FileClipboardData>> {
            let mut paths = Vec::new();
            let mut action = FileClipboardAction::Copy;

            // Try to read from OS clipboard first (may fail or be empty)
            if let Ok(_guard) = ClipboardGuard::open() {
                unsafe {
                    if let Ok(h) = GetClipboardData(CF_HDROP) {
                        if !h.0.is_null() {
                            let hdrop = HDROP(h.0 as *mut _);
                            let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);

                            for i in 0..count {
                                let len = DragQueryFileW(hdrop, i, None);
                                if len == 0 {
                                    continue;
                                }
                                let mut buf: Vec<u16> =
                                    std::iter::repeat_n(0, len as usize + 1).collect();
                                let written = DragQueryFileW(hdrop, i, Some(&mut buf));
                                if written == 0 {
                                    continue;
                                }
                                if let Some(pos) = buf.iter().position(|&c| c == 0) {
                                    buf.truncate(pos);
                                }
                                let os_str = std::ffi::OsString::from_wide(&buf);
                                paths.push(PathBuf::from(os_str));
                            }

                            let format = RegisterClipboardFormatW(w!("Preferred DropEffect"));
                            if format != 0
                                && let Ok(hmem) = GetClipboardData(format)
                                && let Some(effect) = read_u32_from_hglobal(hmem.0 as isize)
                                && let Some(a) = from_drop_effect(effect)
                            {
                                action = a;
                            }
                        }
                    }
                }
            }

            // Check cache first
            if let Some(cache_data) = self.cache.lock().unwrap().clone() {
                // Simple heuristic: if the number of paths matches, and the first path matches,
                // we assume it's the same data and prefer the cache (preserving provider/remote paths).
                if cache_data.paths.len() == paths.len() {
                    if paths.is_empty() {
                        return Ok(Some(cache_data));
                    }
                    if let (Some(c_first), Some(p_first)) =
                        (cache_data.paths.first(), paths.first())
                    {
                        let c_str = c_first.to_string_lossy().to_lowercase().replace("/", "\\");
                        let p_str = p_first.to_string_lossy().to_lowercase().replace("/", "\\");
                        let c_norm = c_str.strip_prefix(r"\\?\").unwrap_or(&c_str);
                        let p_norm = p_str.strip_prefix(r"\\?\").unwrap_or(&p_str);

                        // On Windows, OS might prepend drive letters (C:\) to relative-looking remote paths.
                        // We check if either one is a suffix of the other (normalized).
                        if c_norm == p_norm
                            || (c_norm.len() > 2 && p_norm.ends_with(c_norm))
                            || (p_norm.len() > 2 && c_norm.ends_with(p_norm))
                        {
                            return Ok(Some(cache_data));
                        }
                    }
                }
            }

            if paths.is_empty() {
                Ok(None)
            } else {
                Ok(Some(FileClipboardData {
                    action,
                    paths,
                    source_provider: Arc::new(crate::fs_local::LocalFs::new()),
                }))
            }
        }

        fn clear(&mut self) -> anyhow::Result<()> {
            *self.cache.lock().unwrap() = None;
            let _guard = ClipboardGuard::open()?;
            unsafe {
                EmptyClipboard().context("EmptyClipboard failed")?;
            }
            Ok(())
        }
    }
}

pub enum ClipboardBackend {
    #[cfg(target_os = "windows")]
    Windows(win_clipboard::WindowsFileClipboard),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Unix(unix_clipboard::UnixFileClipboard),
}

impl ClipboardBackend {
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
mod tests {
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn test_unix_clipboard() {
        use super::*;
        let mut cb = unix_clipboard::UnixFileClipboard::new();
        assert!(cb.get().unwrap().is_none());

        let provider = Arc::new(crate::fs_local::LocalFs::new());
        let data = FileClipboardData {
            action: FileClipboardAction::Copy,
            paths: vec![PathBuf::from("/test/file")],
            source_provider: provider.clone(),
        };
        cb.set(data.clone()).unwrap();

        let got = cb.get().unwrap().unwrap();
        assert_eq!(got.action, FileClipboardAction::Copy);
        assert_eq!(got.paths, vec![PathBuf::from("/test/file")]);

        let data_cut = FileClipboardData {
            action: FileClipboardAction::Cut,
            paths: vec![PathBuf::from("/test/move")],
            source_provider: provider,
        };
        cb.set(data_cut).unwrap();
        assert_eq!(cb.get().unwrap().unwrap().action, FileClipboardAction::Cut);
    }
}
