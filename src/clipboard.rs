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
    use std::iter;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    use windows::Win32::UI::Shell::*;
    use windows::core::w;

    const DROPEFFECT_COPY: u32 = 1;
    const DROPEFFECT_MOVE: u32 = 2;

    pub struct WindowsFileClipboard;

    impl WindowsFileClipboard {
        pub fn new() -> Self {
            Self
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
        struct DROPFILES {
            p_files: u32,
            pt_x: i32,
            pt_y: i32,
            f_nc: i32,   // Replacing BOOL (4 bytes) with i32
            f_wide: i32, // Replacing BOOL (4 bytes) with i32
        }

        let header = DROPFILES {
            p_files: std::mem::size_of::<DROPFILES>() as u32,
            pt_x: 0,
            pt_y: 0,
            f_nc: 0,
            f_wide: 1,
        };

        let mut buf = Vec::with_capacity(std::mem::size_of::<DROPFILES>() + wide.len() * 2);

        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                &header as *const DROPFILES as *const u8,
                std::mem::size_of::<DROPFILES>(),
            )
        };
        buf.extend_from_slice(header_bytes);

        for w in wide {
            buf.extend_from_slice(&w.to_le_bytes());
        }

        buf
    }

    unsafe fn alloc_global_from_bytes(bytes: &[u8]) -> anyhow::Result<isize> {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len())
            .map_err(|e| anyhow::anyhow!("GlobalAlloc failed: {}", e))?;
        if hglobal.0.is_null() {
            anyhow::bail!("GlobalAlloc failed");
        }

        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            let _ = GlobalFree(hglobal);
            anyhow::bail!("GlobalLock failed");
        }

        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());

        let _ = GlobalUnlock(hglobal);
        Ok(hglobal.0 as isize)
    }

    unsafe fn read_u32_from_hglobal(hglobal: isize) -> Option<u32> {
        if hglobal == 0 {
            return None;
        }
        let h = HANDLE(hglobal as *mut _);
        let ptr = GlobalLock(h);
        if ptr.is_null() {
            return None;
        }
        let value = *(ptr as *const u32);
        let _ = GlobalUnlock(h);
        Some(value)
    }

    impl FileClipboard for WindowsFileClipboard {
        fn set(&mut self, data: FileClipboardData) -> anyhow::Result<()> {
            unsafe {
                OpenClipboard(None).context("OpenClipboard failed")?;
                let _res = EmptyClipboard();

                let buf = paths_to_dropfiles_buffer(&data.paths);
                let hglobal = alloc_global_from_bytes(&buf)?;
                let _ = SetClipboardData(CF_HDROP.0, Some(HANDLE(hglobal as *mut _)));

                let format = RegisterClipboardFormatW(w!("Preferred DropEffect"));
                if format != 0 {
                    let effect = to_drop_effect(data.action);
                    let hglobal_effect =
                        GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, std::mem::size_of::<u32>())
                            .map_err(|e| {
                                anyhow::anyhow!("GlobalAlloc for DropEffect failed: {}", e)
                            })?;

                    if !hglobal_effect.0.is_null() {
                        let ptr = GlobalLock(hglobal_effect);
                        if !ptr.is_null() {
                            *(ptr as *mut u32) = effect;
                            let _ = GlobalUnlock(hglobal_effect);
                            let _ = SetClipboardData(format, Some(hglobal_effect));
                        }
                    }
                }

                CloseClipboard().context("CloseClipboard failed")?;
            }
            Ok(())
        }

        fn get(&mut self) -> anyhow::Result<Option<FileClipboardData>> {
            unsafe {
                if OpenClipboard(None).is_err() {
                    return Ok(None);
                }

                let hdrop_data = GetClipboardData(CF_HDROP.0);
                if hdrop_data.is_err() {
                    let _ = CloseClipboard();
                    return Ok(None);
                }
                let hdrop = hdrop_data.unwrap();

                let count = DragQueryFileW(HDROP(hdrop.0 as *mut _), 0xFFFFFFFF, None);
                if count == 0 {
                    let _ = CloseClipboard();
                    return Ok(None);
                }

                let mut paths = Vec::new();
                for i in 0..count {
                    let len = DragQueryFileW(HDROP(hdrop.0 as *mut _), i, None);
                    if len == 0 {
                        continue;
                    }
                    let mut buf: Vec<u16> = iter::repeat(0).take(len as usize + 1).collect();
                    let written = DragQueryFileW(HDROP(hdrop.0 as *mut _), i, Some(&mut buf));
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
                let mut action = FileClipboardAction::Copy;
                if format != 0 {
                    if let Ok(hmem) = GetClipboardData(format) {
                        if let Some(effect) = read_u32_from_hglobal(hmem.0 as isize) {
                            if let Some(a) = from_drop_effect(effect) {
                                action = a;
                            }
                        }
                    }
                }

                let _ = CloseClipboard();

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
        }

        fn clear(&mut self) -> anyhow::Result<()> {
            unsafe {
                OpenClipboard(None).context("OpenClipboard failed")?;
                let _res = EmptyClipboard();
                CloseClipboard().context("CloseClipboard failed")?;
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
    use super::*;

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn test_unix_clipboard() {
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
