use super::*;
use std::io::BufRead;
use windows::Win32::Foundation::GetLastError;

fn test_data() -> FileClipboardData {
    FileClipboardData {
        action: FileClipboardAction::Copy,
        paths: vec![PathBuf::from("C:\\temp\\clipboard-test.txt")],
        source_provider: Arc::new(crate::fs::local::LocalFs::new()),
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
