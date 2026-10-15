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
                source_provider: Arc::new(crate::fs::local::LocalFs::new()),
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
mod tests;
