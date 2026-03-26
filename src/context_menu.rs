//! Windows Shell context menu integration.
//!
//! Displays the native Windows Explorer right-click context menu for a local
//! file or directory.  The menu appears at the actual mouse cursor position
//! (obtained via `GetCursorPos`), so no terminal-cell-to-pixel conversion is
//! needed.

#![allow(clippy::cast_possible_truncation)]

use anyhow::{Result, anyhow};
use std::path::Path;

use windows::{
    Win32::{
        Foundation::{HWND, POINT},
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        System::Threading::{AttachThreadInput, GetCurrentThreadId},
        UI::{
            Shell::{
                CMF_NORMAL, CMIC_MASK_PTINVOKE, CMINVOKECOMMANDINFOEX, GCS_VERBA, IContextMenu,
                IShellFolder, SHBindToParent, SHParseDisplayName,
            },
            WindowsAndMessaging::{
                CreatePopupMenu, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos,
                GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, MSG, PostMessageW,
                SW_SHOWNORMAL, SetForegroundWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
                TrackPopupMenu, TranslateMessage, WM_NULL,
            },
        },
    },
    core::{PCWSTR, PSTR},
};

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
#[allow(clippy::cast_sign_loss)]
unsafe fn inner_show(wide_path: &[u16]) -> Result<()> {
    unsafe {
        let (_free_pidl, ctx_menu) = get_context_menu_for_path(wide_path)?;

        let hmenu = CreatePopupMenu().map_err(|e| anyhow!("CreatePopupMenu: {e}"))?;
        ctx_menu
            .QueryContextMenu(hmenu, 0, 1, 0x7FFF, CMF_NORMAL)
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

        // User dismissed without selecting anything.
        if cmd.0 == 0 {
            let _ = DestroyWindow(hwnd_dummy);
            return Ok(());
        }

        let verb_offset = (cmd.0 - 1) as usize;
        let is_properties = is_properties_verb(&ctx_menu, verb_offset);

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

        // If "Properties" was clicked, keep the thread alive indefinitely for the Explorer dialog.
        if is_properties {
            pump_messages_indefinitely();
        }

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

unsafe fn is_properties_verb(ctx_menu: &IContextMenu, verb_offset: usize) -> bool {
    unsafe {
        let mut verb_buf = [0u8; 256];
        if ctx_menu
            .GetCommandString(
                verb_offset,
                GCS_VERBA,
                None,
                PSTR(verb_buf.as_mut_ptr().cast::<u8>()),
                verb_buf.len() as u32,
            )
            .is_ok()
            && let Ok(s) = std::ffi::CStr::from_bytes_until_nul(&verb_buf)
            && s.to_string_lossy().to_lowercase() == "properties"
        {
            return true;
        }
        false
    }
}

unsafe fn pump_messages_indefinitely() {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&raw mut msg, Some(HWND(std::ptr::null_mut())), 0, 0).into() {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
    }
}
