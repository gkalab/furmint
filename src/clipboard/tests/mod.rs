#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn test_unix_clipboard() {
    use super::*;
    let mut cb = unix_clipboard::UnixFileClipboard::new();
    assert!(cb.get().unwrap().is_none());

    let provider = Arc::new(crate::fs::local::LocalFs::new());
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
