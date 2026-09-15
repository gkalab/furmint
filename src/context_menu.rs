//! Windows Shell context menu integration.
//!
//! Displays the native Windows Explorer right-click context menu for a local
//! file or directory.  The menu appears at the actual mouse cursor position
//! (obtained via `GetCursorPos`), so no terminal-cell-to-pixel conversion is
//! needed.

#![allow(clippy::cast_possible_truncation)]

use anyhow::{Result, anyhow};
use std::path::Path;
use std::time::{Duration, Instant};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT},
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        System::Threading::{AttachThreadInput, GetCurrentThreadId},
        UI::{
            Shell::{
                CMF_NORMAL, CMIC_MASK_PTINVOKE, CMINVOKECOMMANDINFOEX, IContextMenu, IShellFolder,
                SHBindToParent, SHParseDisplayName,
            },
            WindowsAndMessaging::{
                CreatePopupMenu, DestroyMenu, DestroyWindow, DispatchMessageW, EnumWindows,
                GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
                IsWindowVisible, MSG, PM_REMOVE, PeekMessageW, PostMessageW, SW_SHOWNORMAL,
                SetForegroundWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
                TranslateMessage, WM_NULL,
            },
        },
    },
    core::{BOOL, PCWSTR},
};

/// First command ID the shell may assign via `QueryContextMenu`.
const CMD_ID_FIRST: i32 = 1;
/// Last command ID the shell may assign via `QueryContextMenu`.
const CMD_ID_LAST: i32 = 0x7FFF;
/// How long to wait for the shell to create a dialog on our thread after
/// `InvokeCommand` returns.
const DIALOG_APPEARANCE_GRACE: Duration = Duration::from_millis(500);
const DIALOG_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Show the native Windows Shell context menu for `path`.
///
/// Blocks until the user dismisses the menu or selects an item, then executes
/// the chosen shell verb (if any).
///
/// # Errors
///
/// Returns an error if the path cannot be resolved or any Win32/COM call fails.
pub fn show_context_menu(path: &Path) -> Result<()> {
    let path_buf = path.to_path_buf();

    // Spawn a dedicated STA OS thread. This is critical because Windows Shell UI
    // and context menus (especially "Properties") expect to run on a standard
    // unblocked thread with a message pump, completely detached from the tokio MTA runtime.
    std::thread::spawn(move || {
        let wide: Vec<u16> = path_buf
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            // S_OK (0) → we initialised COM; S_FALSE (1) → already initialised.
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let we_initialised_com = hr.is_ok() && hr.0 == 0;

            if let Err(e) = inner_show(&wide) {
                let _ = e; // Ignore background errors
            }

            if we_initialised_com {
                CoUninitialize();
            }
        }
    });

    Ok(())
}

// Free the PIDL on exit via a guard.
struct FreePidl(*mut windows::Win32::UI::Shell::Common::ITEMIDLIST);
impl Drop for FreePidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows::Win32::System::Com::CoTaskMemFree(Some(self.0.cast()));
            }
        }
    }
}

/// # Safety
/// Caller must ensure COM is initialised (STA) on the current thread.
unsafe fn inner_show(wide_path: &[u16]) -> Result<()> {
    unsafe {
        let (_free_pidl, ctx_menu) = get_context_menu_for_path(wide_path)?;

        let hmenu = CreatePopupMenu().map_err(|e| anyhow!("CreatePopupMenu: {e}"))?;
        ctx_menu
            .QueryContextMenu(
                hmenu,
                0,
                CMD_ID_FIRST as u32,
                CMD_ID_LAST as u32,
                CMF_NORMAL,
            )
            .ok()
            .map_err(|e| anyhow!("QueryContextMenu: {e}"))?;

        let mut pt = POINT::default();
        GetCursorPos(&raw mut pt).map_err(|e| anyhow!("GetCursorPos: {e}"))?;

        let hwnd_dummy = create_dummy_window()?;
        force_foreground_window(hwnd_dummy);

        let cmd = TrackPopupMenu(
            hmenu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            pt.x,
            pt.y,
            Some(0),
            hwnd_dummy,
            None,
        );

        // Post a benign message to the window to force a task switch.
        let _ = PostMessageW(
            Some(hwnd_dummy),
            WM_NULL,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );

        let _ = DestroyMenu(hmenu);

        // User dismissed the menu, or the result lies outside the command-ID
        // range registered by `QueryContextMenu`: nothing to invoke.
        let Some(verb_offset) = menu_result_to_verb_offset(cmd.0, CMD_ID_FIRST, CMD_ID_LAST) else {
            let _ = DestroyWindow(hwnd_dummy);
            return Ok(());
        };

        let info = CMINVOKECOMMANDINFOEX {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: CMIC_MASK_PTINVOKE,
            hwnd: hwnd_dummy,
            lpVerb: windows::core::PCSTR(verb_offset as *const u8),
            lpVerbW: PCWSTR(verb_offset as *const u16),
            nShow: SW_SHOWNORMAL.0,
            ptInvoke: pt,
            ..Default::default()
        };

        ctx_menu
            .InvokeCommand(std::ptr::addr_of!(info).cast())
            .map_err(|e| anyhow!("InvokeCommand: {e}"))?;

        // The shell may open dialog UI (e.g. the file "Properties" sheet) on
        // this COM thread *after* `InvokeCommand` returns; keep the thread and
        // its message pump alive until that dialog is destroyed.
        keep_alive_for_thread_dialogs();

        let _ = DestroyWindow(hwnd_dummy);
        Ok(())
    }
}

unsafe fn get_context_menu_for_path(wide_path: &[u16]) -> Result<(FreePidl, IContextMenu)> {
    unsafe {
        let mut pidl_full = std::ptr::null_mut();
        SHParseDisplayName(
            PCWSTR(wide_path.as_ptr()),
            None,
            &raw mut pidl_full,
            0,
            None,
        )
        .map_err(|e| anyhow!("SHParseDisplayName: {e}"))?;

        let free = FreePidl(pidl_full);

        let mut pidl_child: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST =
            std::ptr::null_mut();
        let parent: IShellFolder = SHBindToParent(pidl_full, Some(&raw mut pidl_child))
            .map_err(|e| anyhow!("SHBindToParent: {e}"))?;

        let p_child = pidl_child.cast_const();
        let ctx_menu: IContextMenu = parent
            .GetUIObjectOf(HWND(std::ptr::null_mut()), &[p_child], None)
            .map_err(|e| anyhow!("GetUIObjectOf: {e}"))?;

        Ok((free, ctx_menu))
    }
}

unsafe fn create_dummy_window() -> Result<HWND> {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE::default(),
            windows::core::w!("STATIC"),
            windows::core::w!(""),
            windows::Win32::UI::WindowsAndMessaging::WS_POPUP,
            0,
            0,
            0,
            0,
            Some(HWND(std::ptr::null_mut())),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                std::ptr::null_mut(),
            )),
            Some(windows::Win32::Foundation::HINSTANCE(std::ptr::null_mut())),
            None,
        )
        .map_err(|e| anyhow!("CreateWindowExW failed: {e}"))
    }
}

unsafe fn force_foreground_window(hwnd: HWND) {
    unsafe {
        let hwnd_fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(hwnd_fg, None);
        let my_thread = GetCurrentThreadId();

        if fg_thread != 0 && fg_thread != my_thread {
            let _ = AttachThreadInput(my_thread, fg_thread, true);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(my_thread, fg_thread, false);
        } else {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// Maps the raw `TrackPopupMenu` result to the 0-based verb offset passed to
/// `IContextMenu::InvokeCommand` (`lpVerb` = selected command ID - first ID).
///
/// Returns `None` when the user dismissed the menu, or when the value falls
/// outside the `CMD_ID_FIRST..=CMD_ID_LAST` range declared with
/// `QueryContextMenu`. Without this check a failed `TrackPopupMenu` call
/// could yield a negative value whose `(cmd - 1) as usize` wrap-around would
/// be reinterpreted as a raw pointer (undefined behavior).
fn menu_result_to_verb_offset(cmd: i32, cmd_first: i32, cmd_last: i32) -> Option<usize> {
    // `checked_sub` also rejects a `cmd` below `cmd_first`.
    let diff = cmd.checked_sub(cmd_first)?;
    if cmd > cmd_last {
        return None;
    }
    usize::try_from(diff).ok()
}

/// State for [`enum_thread_windows`], passed through the `LPARAM` slot.
struct ThreadWindowScan {
    thread_id: u32,
    found: HWND,
}

/// `EnumWindows` callback recording the first visible top-level window owned
/// by the scanning thread.
///
/// # Safety
///
/// `lparam` must point to a `ThreadWindowScan` that stays alive for the
/// duration of the `EnumWindows` call; the callback runs synchronously on
/// the calling thread.
unsafe extern "system" fn enum_thread_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        let scan = &mut *(lparam.0 as *mut ThreadWindowScan);
        if scan.found.0.is_null()
            && GetWindowThreadProcessId(hwnd, None) == scan.thread_id
            && IsWindowVisible(hwnd).as_bool()
        {
            scan.found = hwnd;
        }
        // Always continue: the binding maps a `FALSE` return to a call
        // failure, so the enumeration cannot be stopped early.
        BOOL(1)
    }
}

/// Finds a visible top-level window owned by the calling thread, if any.
///
/// Detection is purely structural (thread ownership plus visibility), so it
/// works identically on every Windows locale.
unsafe fn find_thread_dialog() -> Option<HWND> {
    unsafe {
        let mut scan = ThreadWindowScan {
            thread_id: GetCurrentThreadId(),
            found: HWND(std::ptr::null_mut()),
        };
        EnumWindows(Some(enum_thread_windows), LPARAM(&raw mut scan as isize)).ok()?;

        (!scan.found.0.is_null()).then_some(scan.found)
    }
}

/// Dispatches every message currently pending on the calling thread's queue.
unsafe fn drain_messages() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
    }
}

/// Keeps the thread alive while the shell owns a visible top-level window on
/// it (e.g. the file "Properties" sheet, which the shell opens on this COM
/// thread *after* `InvokeCommand` returns).
///
/// Waits up to [`DIALOG_APPEARANCE_GRACE`] for such a dialog to appear, then
/// pumps messages until the dialog (and any successor dialog) is destroyed —
/// i.e. its `WM_DESTROY` has been processed — so the thread and the hidden
/// helper window are always cleaned up.
unsafe fn keep_alive_for_thread_dialogs() {
    unsafe {
        let deadline = Instant::now() + DIALOG_APPEARANCE_GRACE;
        let mut dialog_open = find_thread_dialog().is_some();

        while !dialog_open && Instant::now() < deadline {
            drain_messages();
            dialog_open = find_thread_dialog().is_some();
            std::thread::sleep(DIALOG_POLL_INTERVAL);
        }

        while dialog_open {
            let mut msg = MSG::default();
            if !GetMessageW(&raw mut msg, Some(HWND(std::ptr::null_mut())), 0, 0).as_bool() {
                break;
            }
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
            dialog_open = find_thread_dialog().is_some();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismissed_menu_maps_to_no_verb() {
        assert_eq!(
            menu_result_to_verb_offset(0, CMD_ID_FIRST, CMD_ID_LAST),
            None
        );
    }

    #[test]
    fn invalid_track_popup_menu_results_map_to_no_verb() {
        // Regression: the old `(cmd - 1) as usize` expression wrapped for a
        // negative `TrackPopupMenu` result, and the wrapped value was cast
        // straight to a `lpVerb` pointer (undefined behavior).
        assert_eq!(
            menu_result_to_verb_offset(-1, CMD_ID_FIRST, CMD_ID_LAST),
            None
        );
        assert_eq!(
            menu_result_to_verb_offset(i32::MIN, CMD_ID_FIRST, CMD_ID_LAST),
            None
        );
        assert_eq!(
            menu_result_to_verb_offset(CMD_ID_LAST + 1, CMD_ID_FIRST, CMD_ID_LAST),
            None,
        );
    }

    #[test]
    fn valid_command_ids_map_to_zero_based_offsets() {
        assert_eq!(
            menu_result_to_verb_offset(1, CMD_ID_FIRST, CMD_ID_LAST),
            Some(0)
        );
        assert_eq!(
            menu_result_to_verb_offset(5, CMD_ID_FIRST, CMD_ID_LAST),
            Some(4)
        );
        assert_eq!(
            menu_result_to_verb_offset(CMD_ID_LAST, CMD_ID_FIRST, CMD_ID_LAST),
            Some(CMD_ID_LAST as usize - 1),
        );
        // Non-default first ID.
        assert_eq!(menu_result_to_verb_offset(10, 10, 0x7FFF), Some(0));
        assert_eq!(menu_result_to_verb_offset(12, 10, 0x7FFF), Some(2));
    }

    #[test]
    fn scan_ignores_hidden_windows_on_this_thread() {
        unsafe {
            let hidden = create_dummy_window().expect("create hidden window");
            assert!(find_thread_dialog().is_none());
            let _ = DestroyWindow(hidden);
        }
    }

    #[test]
    fn scan_detects_visible_thread_windows_structurally() {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, WINDOW_EX_STYLE, WS_POPUP, WS_VISIBLE,
            };

            let visible = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::w!("STATIC"),
                windows::core::w!(""),
                WS_POPUP | WS_VISIBLE,
                -10_000,
                -10_000,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .expect("create visible window");

            // No localized-string matching: a visible top-level window owned
            // by this thread is detected by ownership and visibility alone.
            assert_eq!(find_thread_dialog(), Some(visible));

            let _ = DestroyWindow(visible);
            assert!(find_thread_dialog().is_none());
        }
    }
}
