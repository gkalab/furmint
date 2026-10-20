/// Empties the user trash. Returns the number of removed items on supported
/// platforms (0 on Windows, where the API reports no count), or an error.
///
/// # Errors
///
/// Returns an error if the trash cannot be emptied.
pub async fn empty_trash() -> std::result::Result<usize, String> {
    tokio::task::spawn_blocking(empty_trash_sync)
        .await
        .map_err(|e| format!("Empty trash task error: {e}"))?
}

/// Blocking implementation of [`empty_trash`], meant to run off the runtime
/// worker threads (e.g. via `tokio::task::spawn_blocking`).
fn empty_trash_sync() -> std::result::Result<usize, String> {
    #[cfg(target_os = "windows")]
    {
        unsafe {
            use winapi::shared::windef::HWND;
            use winapi::shared::winerror::SUCCEEDED;
            use winapi::um::shellapi::{
                SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW,
            };

            let hwnd: HWND = std::ptr::null_mut();
            let psz_root: *const u16 = std::ptr::null(); // all drives

            let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
            let res = SHEmptyRecycleBinW(hwnd, psz_root, flags);

            // E_UNEXPECTED (0x8000ffff) is returned when the Recycle Bin is already empty.
            if SUCCEEDED(res) || res == winapi::shared::winerror::E_UNEXPECTED {
                Ok(0)
            } else {
                Err(format!("Failed: SHEmptyRecycleBinW error code {res:#x}"))
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::env;
        use std::fs;
        use std::path::PathBuf;
        let home = env::var("HOME").map_err(|e| format!("No HOME: {e}"))?;
        let trash_dir = PathBuf::from(format!("{}/.Trash", home));
        if !trash_dir.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in walkdir::WalkDir::new(&trash_dir).min_depth(1).max_depth(1) {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(path).map_err(|e| e.to_string())?;
            } else {
                fs::remove_file(path).map_err(|e| e.to_string())?;
            }
            removed += 1;
        }
        Ok(removed)
    }
    #[cfg(target_os = "linux")]
    {
        use std::env;
        use std::fs;
        use std::path::PathBuf;
        let home = env::var("HOME").map_err(|e| format!("No HOME: {e}"))?;
        let base = PathBuf::from(format!("{home}/.local/share/Trash"));
        let files = base.join("files");
        let info = base.join("info");
        let mut removed = 0;
        for dir in &[&files, &info] {
            if dir.exists() {
                for entry in walkdir::WalkDir::new(dir).min_depth(1).max_depth(1) {
                    let Ok(entry) = entry else { continue };
                    let path = entry.path();
                    if path.is_dir() {
                        fs::remove_dir_all(path).map_err(|e| e.to_string())?;
                    } else {
                        fs::remove_file(path).map_err(|e| e.to_string())?;
                    }
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err("Not supported on this OS".to_string())
    }
}
