use crate::fs::fs_provider::FileSystemProvider;
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
pub mod win_clipboard {
    use super::{FileClipboard, FileClipboardAction, FileClipboardData};
    use anyhow::Context;
    use std::ffi::OsStr;

    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
        SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
    };
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows::core::w;

    const CF_HDROP: u32 = 15;

    const DROPEFFECT_COPY: u32 = 1;
    const DROPEFFECT_MOVE: u32 = 2;

    pub struct WindowsFileClipboard {
        cache: Arc<Mutex<Option<FileClipboardData>>>,
    }

    impl Default for WindowsFileClipboard {
        fn default() -> Self {
            Self::new()
        }
    }

    impl WindowsFileClipboard {
        /// Total time a clipboard write may block, including retries on a locked clipboard.
        const SET_BUDGET: Duration = Duration::from_millis(500);
        /// Total time a clipboard read may block while the clipboard is locked.
        const READ_BUDGET: Duration = Duration::from_millis(300);

        #[must_use]
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
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Dropfiles {
            p_files: u32,
            pt_x: i32,
            pt_y: i32,
            f_nc: i32,   // Replacing BOOL (4 bytes) with i32
            f_wide: i32, // Replacing BOOL (4 bytes) with i32
        }

        let mut wide: Vec<u16> = Vec::new();
        for p in paths {
            let s: &OsStr = p.as_os_str();
            let mut v: Vec<u16> = s.encode_wide().collect();
            v.push(0);
            wide.extend_from_slice(&v);
        }
        wide.push(0);

        let header = Dropfiles {
            #[allow(clippy::cast_possible_truncation)]
            p_files: std::mem::size_of::<Dropfiles>() as u32,
            pt_x: 0,
            pt_y: 0,
            f_nc: 0,
            f_wide: 1,
        };

        let mut buf = Vec::with_capacity(std::mem::size_of::<Dropfiles>() + wide.len() * 2);

        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                (&raw const header).cast::<u8>(),
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
                .map_err(|e| anyhow::anyhow!("GlobalAlloc failed: {e}"))?
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
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(hglobal);
        }

        Ok(hglobal.0 as isize)
    }

    unsafe fn alloc_drop_effect(effect: u32) -> anyhow::Result<isize> {
        let hglobal = unsafe {
            GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, std::mem::size_of::<u32>())
                .map_err(|e| anyhow::anyhow!("GlobalAlloc for DropEffect failed: {e}"))?
        };
        if hglobal.0.is_null() {
            anyhow::bail!("GlobalAlloc for DropEffect failed");
        }
        let ptr = unsafe { GlobalLock(hglobal) };
        if ptr.is_null() {
            unsafe {
                let _ = GlobalFree(Some(hglobal));
            }
            anyhow::bail!("GlobalLock for DropEffect failed");
        }
        unsafe {
            *ptr.cast::<u32>() = effect;
            let _ = GlobalUnlock(hglobal);
        }
        Ok(hglobal.0 as isize)
    }

    /// Frees a global memory block the clipboard did not take ownership of.
    ///
    /// # Safety
    ///
    /// `handle` must be a valid HGLOBAL not owned by the clipboard, or zero.
    unsafe fn free_global(handle: isize) {
        if handle != 0 {
            unsafe {
                let _ = GlobalFree(Some(HGLOBAL(handle as *mut _)));
            }
        }
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
        let size = unsafe { GlobalSize(h) };
        if size < std::mem::size_of::<u32>() {
            unsafe {
                let _ = GlobalUnlock(h);
            }
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
        /// Opens the clipboard, retrying with exponential backoff until `deadline` while it is
        /// locked by another owner. The total wait is bounded by the deadline.
        fn open_until(deadline: Instant) -> anyhow::Result<Self> {
            let mut backoff = Duration::from_millis(10);
            loop {
                if unsafe { OpenClipboard(None) }.is_ok() {
                    return Ok(Self);
                }
                let now = Instant::now();
                if now >= deadline {
                    anyhow::bail!("OpenClipboard failed: clipboard is locked by another process");
                }
                std::thread::sleep(backoff.min(deadline - now));
                backoff = (backoff * 2).min(Duration::from_millis(100));
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

    /// Frees both buffers when neither has been handed to the clipboard.
    unsafe fn free_unplaced(hdrop: isize, effect_handle: Option<isize>) {
        unsafe {
            free_global(hdrop);
            if let Some(h) = effect_handle {
                free_global(h);
            }
        }
    }

    /// Opens the clipboard, empties it, and places the pre-allocated buffers.
    ///
    /// On any failure, frees every buffer the clipboard did not take ownership of.
    /// Once `SetClipboardData` succeeds for a buffer, the clipboard owns it.
    unsafe fn place_clipboard_data(
        hdrop: isize,
        effect_format: u32,
        effect_handle: Option<isize>,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        let _guard = match ClipboardGuard::open_until(deadline) {
            Ok(guard) => guard,
            Err(e) => {
                unsafe {
                    free_unplaced(hdrop, effect_handle);
                }
                return Err(e);
            }
        };

        if let Err(e) = unsafe { EmptyClipboard() } {
            unsafe {
                free_unplaced(hdrop, effect_handle);
            }
            return Err(anyhow::Error::new(e).context("EmptyClipboard failed"));
        }

        if let Err(e) = unsafe { SetClipboardData(CF_HDROP, Some(HANDLE(hdrop as *mut _))) } {
            // The clipboard did not take ownership of the HDROP buffer.
            unsafe {
                free_unplaced(hdrop, effect_handle);
            }
            return Err(anyhow::Error::new(e).context("SetClipboardData CF_HDROP failed"));
        }
        // From here on the clipboard owns `hdrop`; only the DropEffect buffer may be freed.

        if effect_format != 0
            && let Some(h) = effect_handle
            && let Err(e) = unsafe { SetClipboardData(effect_format, Some(HANDLE(h as *mut _))) }
        {
            unsafe {
                free_global(h);
            }
            return Err(anyhow::Error::new(e).context("SetClipboardData DropEffect failed"));
        }
        Ok(())
    }

    impl WindowsFileClipboard {
        fn try_set_clipboard(data: &FileClipboardData, deadline: Instant) -> anyhow::Result<()> {
            // Allocate all data before opening the clipboard so it is only held while writing.
            let buf = paths_to_dropfiles_buffer(&data.paths);
            let hdrop = unsafe { alloc_global_from_bytes(&buf) }?;
            let effect_format = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
            let effect_handle = if effect_format != 0 {
                Some(unsafe { alloc_drop_effect(to_drop_effect(data.action)) }?)
            } else {
                None
            };
            unsafe { place_clipboard_data(hdrop, effect_format, effect_handle, deadline) }
        }
    }

    impl FileClipboard for WindowsFileClipboard {
        fn set(&mut self, data: FileClipboardData) -> anyhow::Result<()> {
            // Update in-process cache
            *self.cache.lock().unwrap() = Some(data.clone());

            // Bound the total wait (open retries + write) so the UI thread is never
            // blocked for more than SET_BUDGET even while the clipboard is locked.
            let deadline = Instant::now() + Self::SET_BUDGET;
            Self::try_set_clipboard(&data, deadline)
        }

        fn get(&mut self) -> anyhow::Result<Option<FileClipboardData>> {
            let mut paths = Vec::new();
            let mut action = FileClipboardAction::Copy;

            // Try to read from OS clipboard (e.g. from Explorer)
            let read_deadline = Instant::now() + Self::READ_BUDGET;
            if let Ok(_guard) = ClipboardGuard::open_until(read_deadline) {
                unsafe {
                    // ifs must not be collapsed
                    #[allow(clippy::collapsible_if)]
                    if let Ok(h) = GetClipboardData(CF_HDROP) {
                        if !h.0.is_null() {
                            let hdrop = HDROP(h.0.cast());
                            let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);

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

            // Check if OS clipboard matches our cache
            if let Some(cache_data) = self.cache.lock().unwrap().clone()
                && cache_data.paths == paths
            {
                return Ok(Some(cache_data));
            }

            if paths.is_empty() {
                Ok(None)
            } else {
                Ok(Some(FileClipboardData {
                    action,
                    paths,
                    source_provider: Arc::new(crate::fs::fs_local::LocalFs::new()),
                }))
            }
        }

        fn clear(&mut self) -> anyhow::Result<()> {
            *self.cache.lock().unwrap() = None;
            let deadline = Instant::now() + Self::SET_BUDGET;
            let _guard = ClipboardGuard::open_until(deadline)?;
            unsafe {
                EmptyClipboard().context("EmptyClipboard failed")?;
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::BufRead;
        use windows::Win32::Foundation::GetLastError;

        fn test_data() -> FileClipboardData {
            FileClipboardData {
                action: FileClipboardAction::Copy,
                paths: vec![PathBuf::from("C:\\temp\\clipboard-test.txt")],
                source_provider: Arc::new(crate::fs::fs_local::LocalFs::new()),
            }
        }

        /// Regression test: a locked clipboard used to cost up to ~6-7 s of blocking retries.
        #[test]
        fn set_is_bounded_when_clipboard_is_locked() {
            // A separate process must own the clipboard the legacy way (real window handle):
            // modern-mode `OpenClipboard(0)` calls may succeed concurrently on recent Windows.
            let script = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool OpenClipboard(int hWnd);
[DllImport("user32.dll")] public static extern bool CloseClipboard();
'@ -Name W32 -Namespace Clip
$f = New-Object System.Windows.Forms.Form
$f.Visible = $false
$ok = $false
for ($i = 0; $i -lt 50; $i++) {
    $ok = [Clip.W32]::OpenClipboard([int]$f.Handle)
    if ($ok) { break }
    Start-Sleep -Milliseconds 100
}
if ($ok) {
    Write-Output "HOLDING"
    Start-Sleep -Milliseconds 8000
    [Clip.W32]::CloseClipboard() | Out-Null
}
"#;
            let Ok(mut child) = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", script])
                .stdout(std::process::Stdio::piped())
                .spawn()
            else {
                return; // no PowerShell in this environment: skip
            };

            // Wait for the holder to confirm it owns the clipboard (EOF means skip).
            let mut held = false;
            if let Some(out) = child.stdout.as_mut() {
                let mut reader = std::io::BufReader::new(out);
                let mut line = String::new();
                held = reader.read_line(&mut line).is_ok() && line.contains("HOLDING");
            }
            if !held {
                let _ = child.wait();
                return;
            }

            let mut cb = WindowsFileClipboard::new();
            let start = Instant::now();
            let result = cb.set(test_data());
            let elapsed = start.elapsed();

            assert!(
                result.is_err(),
                "set() must fail while the clipboard is locked"
            );
            assert!(
                elapsed <= Duration::from_secs(1),
                "set() blocked for {elapsed:?}; clipboard writes must finish within a few hundred ms"
            );

            let _ = child.wait();
        }

        /// Verifies the alloc/free helpers used by the leak fix on a `SetClipboardData` failure.
        #[test]
        fn global_alloc_free_roundtrip() {
            let buf = paths_to_dropfiles_buffer(&[
                PathBuf::from("C:\\temp\\a.txt"),
                PathBuf::from("C:\\temp\\b.txt"),
            ]);
            let handle = unsafe { alloc_global_from_bytes(&buf) }.expect("GlobalAlloc failed");
            assert_ne!(handle, 0);

            let h = HGLOBAL(handle as *mut _);
            assert_eq!(unsafe { GlobalSize(h) }, buf.len());
            let ptr = unsafe { GlobalLock(h) };
            assert!(!ptr.is_null());
            let slice = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), buf.len()) };
            assert_eq!(slice, buf.as_slice());
            let _ = unsafe { GlobalUnlock(h) };

            // The leak fix relies on this handle being freed when the clipboard refuses
            // the data. The crate's GlobalFree wrapper reports a NULL return as an error,
            // but the API returns NULL ("previous handle") on success, so verify with
            // the thread's last-error value instead.
            let _ = unsafe { GlobalFree(Some(h)) };
            assert_eq!(
                unsafe { GetLastError() },
                windows::Win32::Foundation::WIN32_ERROR(0)
            );

            // Zero must be a no-op so error paths can call it unconditionally.
            unsafe { free_global(0) };
        }
    }
}

/// In-memory clipboard for tests - does not use OS clipboard
/// Use this in tests to avoid parallel test interference on Windows
#[derive(Clone, Default)]
pub struct InMemoryFileClipboard {
    inner: Option<FileClipboardData>,
}

impl InMemoryFileClipboard {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

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
mod tests {
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn test_unix_clipboard() {
        use super::*;
        let mut cb = unix_clipboard::UnixFileClipboard::new();
        assert!(cb.get().unwrap().is_none());

        let provider = Arc::new(crate::fs::fs_local::LocalFs::new());
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
