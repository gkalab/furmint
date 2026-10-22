//! Test suite for `app_state::tabs`.
mod search_multi_tab;
mod tabs;

mod app_integration;
mod cursor;
mod file_ops;
mod filtering;
mod visibility;

use crate::app::AppState;
use crate::app_state::tabs::{Tab, TabManager};
use crate::fs::utils::FileEntry;
use crate::handlers::toggle_hidden::handle_toggle_hidden;
use std::path::PathBuf;

/// Attributes of an entry that the hidden-file setting must withhold.
///
/// The filesystems mark hidden entries differently, so the fixture cannot rely on
/// the leading dot alone: a Unix filesystem only has the dot convention, whereas a
/// local Windows one ignores it and reads `FILE_ATTRIBUTE_HIDDEN`, which sits at
/// index 2 of a DOS-style string (`d-h---` for a directory, `--h---` for a file).
fn hidden_attrs(is_dir: bool) -> &'static str {
    if cfg!(windows) {
        if is_dir { "d-h---" } else { "--h---" }
    } else if is_dir {
        "drwxr-xr-x"
    } else {
        "-rw-r--r--"
    }
}

/// A listing with two hidden entries — `.config` and `.env` — next to the plain
/// `docs` and `README.md`, plus the `..` navigation entry. Sorts in the order
/// given.
///
/// The hidden entries are dot-prefixed *and* carry [`hidden_attrs`], which is
/// what makes them hidden on every filesystem.
fn listing() -> Vec<FileEntry> {
    vec![
        entry("..", true, ""),
        entry(".config", true, hidden_attrs(true)),
        entry("docs", true, "drwxr-xr-x"),
        entry(".env", false, hidden_attrs(false)),
        entry("README.md", false, "-rw-r--r--"),
    ]
}

/// A listing shaped like a real remote (SFTP) one: Unix permission strings and
/// no DOS attribute flags, so the leading dot is the only hidden signal there is.
fn remote_listing() -> Vec<FileEntry> {
    vec![
        entry("..", true, ""),
        entry(".config", true, "drwxr-xr-x"),
        entry("docs", true, "drwxr-xr-x"),
        entry(".env", false, "-rw-r--r--"),
        entry("README.md", false, "-rw-r--r--"),
    ]
}

fn entry(name: &str, is_dir: bool, attributes: &str) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: if is_dir { None } else { Some(100) },
        modified: None,
        attributes: attributes.to_string(),
        selected: false,
    }
}

fn tab_with(entries: Vec<FileEntry>) -> Tab {
    let mut tab = crate::test_utils::create_test_tab();
    tab.entries = entries;
    tab.current_dir = PathBuf::from("/tmp");
    tab.recompute_visible_indices();
    tab
}

fn remote_tab_with(entries: Vec<FileEntry>) -> Tab {
    crate::test_utils::create_remote_test_tab(entries)
}

fn names(tab: &Tab) -> Vec<&str> {
    tab.visible_indices
        .iter()
        .map(|&i| tab.entries[i].name.as_str())
        .collect()
}

fn manager(tab: Tab) -> TabManager {
    TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    }
}

fn app_with(entries: Vec<FileEntry>) -> AppState {
    let tab = tab_with(entries);
    crate::test_utils::TestAppBuilder::new()
        .left(manager(tab.clone()))
        .right(manager(tab))
        .build()
}

/// On Windows a leading dot is not a hidden marker; use the DOS flag instead.
#[cfg(windows)]
fn mark_hidden(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let rc =
        unsafe { winapi::um::fileapi::SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) };
    assert_eq!(rc, 1, "SetFileAttributesW({path:?}) failed");
}

#[cfg(not(windows))]
fn mark_hidden(_path: &std::path::Path) {}
