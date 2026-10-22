use super::*;

/// A directory preview starts with the directory's full name and lists its
/// children below it, directories first (as `list_dir` orders them).
#[tokio::test]
async fn test_directory_listing_content() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("zeta")).unwrap();
    std::fs::write(temp.path().join("alpha.txt"), "hi").unwrap();

    let provider: std::sync::Arc<dyn crate::fs::provider::FileSystemProvider> =
        std::sync::Arc::new(crate::fs::local::LocalFs::new());
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state
        .load_content(temp.path(), &provider, None, VIEWER_LIMIT, true)
        .await;

    let expected_header = temp.path().to_string_lossy().to_string();
    assert_eq!(state.text.content[0], expected_header);
    assert_eq!(state.text.content[1], "zeta");
    assert_eq!(state.text.content[2], "alpha.txt");
}

/// When hidden files are hidden, the directory preview omits them, matching the
/// tab listing; when shown, they appear like any other entry.
#[tokio::test]
async fn test_directory_listing_respects_hidden_flag() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("alpha.txt"), "hi").unwrap();
    std::fs::write(temp.path().join(".hidden.txt"), "shh").unwrap();
    std::fs::create_dir(temp.path().join(".hidden_dir")).unwrap();
    mark_hidden(&temp.path().join(".hidden.txt"));
    mark_hidden(&temp.path().join(".hidden_dir"));

    let provider: std::sync::Arc<dyn crate::fs::provider::FileSystemProvider> =
        std::sync::Arc::new(crate::fs::local::LocalFs::new());

    let mut hidden = FileViewerState::new(true, "catppuccin macchiato");
    hidden
        .load_content(temp.path(), &provider, None, VIEWER_LIMIT, false)
        .await;
    assert_eq!(hidden.text.content.len(), 2, "header plus one visible file");
    assert_eq!(hidden.text.content[1], "alpha.txt");
    assert!(
        !hidden.text.content.iter().any(|l| l.contains("hidden")),
        "hidden entries must be withheld: {:?}",
        hidden.text.content
    );

    let mut shown = FileViewerState::new(true, "catppuccin macchiato");
    shown
        .load_content(temp.path(), &provider, None, VIEWER_LIMIT, true)
        .await;
    assert!(shown.text.content.iter().any(|l| l == ".hidden.txt"));
    assert!(shown.text.content.iter().any(|l| l == ".hidden_dir"));
}

/// The plain content is only the names; the icons are drawn by the renderer
/// when icons are enabled.
#[test]
fn test_directory_listing_renders_icons() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["/tmp/dir".to_string(), "zeta".to_string()];
    state.text.directory_rows = Some(vec![header_row("/tmp/dir"), dir_row("zeta")]);

    let rendered = render_viewer_with_icons(&mut state);
    assert!(
        rendered.contains('󰉋'),
        "a directory entry must carry its folder icon, got:\n{rendered}"
    );
    assert!(rendered.contains("zeta"));
}

/// Entries are colored with the same foregrounds as a tab listing: directories
/// blue, regular files the default text color, and symlinks cyan.
#[test]
fn test_directory_listing_uses_panel_colors() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec![
        "/tmp/dir".to_string(),
        "subdir".to_string(),
        "plain.txt".to_string(),
        "link".to_string(),
    ];
    state.text.directory_rows = Some(vec![
        header_row("/tmp/dir"),
        dir_row("subdir"),
        file_row("plain.txt"),
        symlink_row("link"),
    ]);

    let (_, colors) = render_viewer_cells(&mut state);
    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let has = |rgb: crate::theme::Rgb| colors.contains(&Color::Rgb(rgb.r, rgb.g, rgb.b));
    assert!(has(palette.blue), "directories must be blue: {colors:?}");
    assert!(has(palette.text), "regular files must use the text color");
    assert!(has(palette.cyan), "symlinks must be cyan");
}

/// Directory-listing rows draw an icon before the text stored in `content`, so
/// mouse hit-testing needs to know how many columns to skip.
#[test]
fn test_directory_listing_rendered_row_prefix() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["/tmp/dir".to_string(), "zeta".to_string()];
    state.text.directory_rows = Some(vec![header_row("/tmp/dir"), dir_row("zeta")]);

    // The header draws no icon, and neither does any row when icons are off.
    assert_eq!(state.rendered_row_prefix(0, true), 0);
    assert_eq!(state.rendered_row_prefix(0, false), 0);
    assert_eq!(state.rendered_row_prefix(1, false), 0);
    // Out-of-range rows have no prefix.
    assert_eq!(state.rendered_row_prefix(9, true), 0);

    let icon_width = crate::icons::get_icon("zeta", true, false, false)
        .chars()
        .count();
    assert_eq!(state.rendered_row_prefix(1, true), icon_width + 1);
}

/// A text selection on a directory listing line is rendered with the same
/// background color the text viewer uses.
#[test]
fn test_directory_listing_renders_text_selection() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["/tmp/dir".to_string(), "alpha.txt".to_string()];
    state.text.directory_rows = Some(vec![header_row("/tmp/dir"), file_row("alpha.txt")]);
    // Select the entry name (content characters 0..9) on the second line.
    state.text.selection = Some(((1, 0), (1, 9)));

    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let surface0 = Color::Rgb(palette.surface0.r, palette.surface0.g, palette.surface0.b);

    let bgs = render_viewer_bg_cells(&mut state, false);
    // Interior starts at (1, 1); content line 1 is screen row 2.
    assert_eq!(
        bgs[2][1], surface0,
        "selection must start at the first char"
    );
    assert_eq!(bgs[2][9], surface0, "selection covers the whole name");
    assert_ne!(bgs[2][10], surface0, "selection must stop at the name");
    assert_ne!(bgs[1][1], surface0, "the header stays unselected");
}

/// With icons enabled the selection highlight is shifted past the icon so it
/// still lines up with the name.
#[test]
fn test_directory_listing_selection_accounts_for_icon_offset() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["/tmp/dir".to_string(), "alpha.txt".to_string()];
    state.text.directory_rows = Some(vec![header_row("/tmp/dir"), file_row("alpha.txt")]);
    state.text.selection = Some(((1, 0), (1, 9)));

    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let surface0 = Color::Rgb(palette.surface0.r, palette.surface0.g, palette.surface0.b);

    let offset = state.rendered_row_prefix(1, true);
    assert!(
        offset >= 2,
        "an entry icon plus its space is at least 2 cells"
    );

    let bgs = render_viewer_bg_cells(&mut state, true);
    let first = 1 + offset;
    assert_eq!(
        bgs[2][first], surface0,
        "selection starts after the icon, not on it"
    );
    assert_eq!(bgs[2][first + 8], surface0, "selection covers the name");
    assert_ne!(bgs[2][1], surface0, "the icon itself is not highlighted");
}

/// Ctrl+C copies the selected lines; for a directory listing that means the
/// entry names exactly as shown, without any icon.
#[test]
fn test_directory_listing_selection_copies_names() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec![
        "/tmp/dir".to_string(),
        "zeta".to_string(),
        "alpha.txt".to_string(),
    ];
    state.text.directory_rows = Some(vec![
        header_row("/tmp/dir"),
        dir_row("zeta"),
        file_row("alpha.txt"),
    ]);
    state.text.selection = Some(((1, 0), (2, 9)));

    assert_eq!(
        state.get_selected_text().as_deref(),
        Some("zeta\nalpha.txt")
    );
}

/// Selecting a symlinked directory shows its link target next to its name.
#[cfg(unix)]
#[tokio::test]
async fn test_directory_listing_shows_link_target() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("child.txt"), "hi").unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let provider: std::sync::Arc<dyn crate::fs::provider::FileSystemProvider> =
        std::sync::Arc::new(crate::fs::local::LocalFs::new());
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state
        .load_content(&link, &provider, None, VIEWER_LIMIT, true)
        .await;

    let expected = format!("{} -> {}", link.to_string_lossy(), real.to_string_lossy());
    assert_eq!(state.text.content[0], expected);
    // The link lists its target's contents.
    assert_eq!(state.text.content[1], "child.txt");
}

const VIEWER_LIMIT: u64 = 10 * 1024 * 1024;

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

fn header_row(name: &str) -> crate::state::DirectoryListingRow {
    crate::state::DirectoryListingRow {
        is_header: true,
        entry: listing_entry(name, true, false),
        hidden: false,
        executable: false,
        link_target: None,
    }
}

/// A directory listing row for a child directory.
fn dir_row(name: &str) -> crate::state::DirectoryListingRow {
    entry_row(name, true, false)
}

/// A directory listing row for a regular file.
fn file_row(name: &str) -> crate::state::DirectoryListingRow {
    entry_row(name, false, false)
}

/// A directory listing row for a symlink.
fn symlink_row(name: &str) -> crate::state::DirectoryListingRow {
    entry_row(name, false, true)
}

/// A non-header row. These tests don't exercise the hidden or executable flags,
/// so both stay unset.
fn entry_row(name: &str, is_dir: bool, is_symlink: bool) -> crate::state::DirectoryListingRow {
    crate::state::DirectoryListingRow {
        is_header: false,
        entry: listing_entry(name, is_dir, is_symlink),
        hidden: false,
        executable: false,
        link_target: None,
    }
}

/// A bare entry for a listing row, with the metadata fields the draw pass does
/// not read left unset.
fn listing_entry(name: &str, is_dir: bool, is_symlink: bool) -> crate::fs::utils::FileEntry {
    crate::fs::utils::FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    }
}
