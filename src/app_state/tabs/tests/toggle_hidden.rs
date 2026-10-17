//! Tests for the global show/hide of hidden files and directories.

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

#[test]
fn test_show_hidden_defaults_to_true() {
    // The documented default, and what a first run or a legacy state file gets.
    let app = app_with(listing());
    assert!(app.show_hidden);
    assert!(!app.active_tab().has_visibility_restriction());
}

#[test]
fn test_app_new_reconciles_tabs_with_the_saved_setting() {
    // Assemble the app, then flip the setting through the same entry point the
    // toggle uses, and confirm every tab follows.
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    for panel in [&app.panels.left, &app.panels.right] {
        assert_eq!(names(panel.active_tab()), vec!["..", "docs", "README.md"]);
    }
}

#[test]
fn test_sorting_rebuilds_the_visible_set() {
    // Reordering entries invalidates `visible_indices`. Use a listing that the
    // sort genuinely reorders, so a stale index list would map a visible slot
    // onto an entry the filter excludes.
    let mut tab = tab_with(vec![
        entry("zeta.txt", false, "-rw-r--r--"),
        entry("alpha.rs", false, "-rw-r--r--"),
        entry("beta", true, "drwxr-xr-x"),
    ]);
    tab.set_file_filter(Some("rs")).unwrap();
    assert_eq!(names(&tab), vec!["alpha.rs", "beta"]);
    tab.cursor = 0; // "alpha.rs", the only filtered-in file

    tab.handle_sort(crate::app_state::tabs::SortColumn::Name);

    for &i in &tab.visible_indices {
        let e = &tab.entries[i];
        let file_matches = tab
            .filter
            .glob
            .as_ref()
            .is_some_and(|g| g.is_match(e.name.as_str()));
        assert!(
            e.is_dir || file_matches,
            "{:?} is marked visible but does not pass the restriction",
            e.name
        );
    }
    assert_eq!(names(&tab), vec!["beta", "alpha.rs"]);
    assert_eq!(
        tab.current_entry().map(|e| e.name.as_str()),
        Some("alpha.rs"),
        "the cursor stays on the same entry across the re-sort"
    );
}

#[test]
fn test_wheel_stepping_skips_hidden_entries() {
    // The mouse wheel steps through the visible listing, so it must never land
    // on a withheld entry.
    let mut tab = tab_with(listing());
    tab.set_show_hidden(false);
    // visible listing: [.., docs, README.md]
    tab.cursor = 1; // "docs", the first visible entry after ".."
    for _ in 0..3 {
        tab.move_cursor_down();
        assert!(
            tab.is_visible(tab.cursor_entry_index().unwrap()),
            "cursor landed on a withheld entry"
        );
    }
    assert_eq!(
        tab.current_entry().map(|e| e.name.as_str()),
        Some("README.md")
    );
}

#[test]
fn test_hidden_gate_excludes_dotfiles_and_dot_directories() {
    let mut tab = tab_with(listing());
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );

    tab.set_show_hidden(false);
    assert_eq!(names(&tab), vec!["..", "docs", "README.md"]);
}

#[test]
fn test_parent_entry_is_never_hidden() {
    let mut tab = tab_with(listing());
    tab.set_show_hidden(false);
    assert!(tab.is_visible(0));
    assert_eq!(tab.entries[0].name, "..");
}

#[test]
fn test_toggle_back_restores_hidden_entries() {
    let mut tab = tab_with(listing());
    tab.set_show_hidden(false);
    tab.set_show_hidden(true);
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );
}

#[test]
fn test_hidden_gate_delegates_to_the_shared_is_hidden_predicate() {
    // The filesystem-dependent difference (leading dot on Unix filesystems, DOS
    // attribute on a local Windows one) must stay in `fs::utils::is_hidden`
    // rather than being reimplemented here.
    let entries = vec![
        entry("..", true, ""),
        // Hidden through the leading dot, which only counts on Unix filesystems.
        entry(".dotfile", false, "-rw-r--r--"),
        // Hidden through the attribute string, which is what counts on Windows.
        entry("flagged", false, hidden_attrs(false)),
        entry("plain.txt", false, "-rw-r--r--"),
    ];
    let mut tab = tab_with(entries);
    let unix_attributes = crate::fs::utils::has_unix_attributes(tab.provider.is_local());
    tab.set_show_hidden(false);

    for i in 0..tab.entries.len() {
        let e = &tab.entries[i];
        assert_eq!(
            tab.is_visible(i),
            e.name == ".." || !crate::fs::utils::is_hidden(&e.name, &e.attributes, unix_attributes),
            "visibility of {:?} disagrees with is_hidden",
            e.name
        );
    }
}

#[test]
fn test_cursor_moves_onto_a_visible_entry() {
    let mut tab = tab_with(listing());
    tab.cursor = 3; // ".env", the last visible row while all entries are shown
    tab.set_show_hidden(false);
    assert!(tab.is_visible(tab.cursor_entry_index().unwrap()));
    assert_eq!(tab.current_entry().map(|e| e.name.as_str()), Some("docs"));
}

#[test]
fn test_cursor_pinned_to_zero_when_nothing_is_visible() {
    let mut tab = tab_with(vec![
        entry("..", true, ""),
        entry(".env", false, hidden_attrs(false)),
    ]);
    tab.cursor = 1;
    tab.set_show_hidden(false);
    // Only ".." survives; an empty set must not leave the cursor on ".env".
    assert_eq!(names(&tab), vec![".."]);
    assert_eq!(tab.cursor, 0);
    assert_eq!(tab.current_entry().map(|e| e.name.as_str()), Some(".."));
}

#[test]
fn test_cursor_pinned_when_filter_matches_nothing() {
    // ".." is a directory and always survives, so use a files-only listing to
    // reach a genuinely empty visible set.
    let mut tab = tab_with(vec![
        entry("a.txt", false, "-rw-r--r--"),
        entry("b.txt", false, "-rw-r--r--"),
    ]);
    tab.cursor = 1; // "b.txt", the last visible row
    tab.set_file_filter(Some("nothing-matches-this")).unwrap();

    assert_eq!(tab.visible_indices, Vec::<usize>::new());
    assert_eq!(
        tab.cursor, 0,
        "an empty visible set must not leave the cursor on a hidden file"
    );
}

#[test]
fn test_selection_of_hidden_entry_is_cleared() {
    let mut tab = tab_with(listing());
    tab.entries[3].selected = true; // ".env"
    tab.entries[4].selected = true; // "README.md"
    tab.set_show_hidden(false);

    assert!(
        !tab.entries[3].selected,
        "hidden entry must not stay selected"
    );
    assert!(tab.entries[4].selected, "visible entry keeps its selection");
    let selected: Vec<&str> = tab
        .get_selected_entries()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(selected, vec!["README.md"]);
}

#[test]
fn test_set_show_hidden_keeps_the_active_filter() {
    let mut tab = tab_with(listing());
    tab.set_file_filter(Some("README")).unwrap();
    tab.set_show_hidden(false);

    assert_eq!(tab.filter.applied.as_deref(), Some("README"));
    assert_eq!(names(&tab), vec!["..", "docs", "README.md"]);
}

#[test]
fn test_filter_cannot_surface_a_hidden_entry() {
    // The hidden gate runs before the glob, so with hidden entries hidden the
    // ".env" match is withheld no matter how specific the pattern. Directories
    // are unaffected by a glob and stay listed.
    let mut tab = tab_with(listing());
    tab.set_show_hidden(false);
    tab.set_file_filter(Some(".env")).unwrap();
    assert_eq!(names(&tab), vec!["..", "docs"]);

    // Showing hidden entries again lets the same filter reach ".env".
    tab.set_show_hidden(true);
    assert_eq!(names(&tab), vec!["..", ".config", "docs", ".env"]);
}

#[test]
fn test_has_visibility_restriction_tracks_both_settings() {
    let mut tab = tab_with(listing());
    assert!(!tab.has_visibility_restriction());

    tab.set_show_hidden(false);
    assert!(tab.has_visibility_restriction());

    tab.set_show_hidden(true);
    assert!(!tab.has_visibility_restriction());

    tab.set_file_filter(Some("README")).unwrap();
    assert!(tab.has_visibility_restriction());
}

#[test]
fn test_listed_file_count_ignores_the_filter_but_not_the_hidden_gate() {
    let mut tab = tab_with(listing());
    assert_eq!(tab.listed_file_count(), 2); // ".env" + "README.md"

    tab.set_show_hidden(false);
    assert_eq!(tab.listed_file_count(), 1); // only "README.md"

    // The filter narrows the listing but must not change the denominator.
    tab.set_file_filter(Some("README")).unwrap();
    assert_eq!(tab.visible_file_count(), 1);
    assert_eq!(tab.listed_file_count(), 1);
}

#[tokio::test]
async fn test_handle_toggle_hidden_applies_to_every_tab_in_both_panels() {
    let left = manager(tab_with(listing()));
    let mut extra = tab_with(listing());
    extra.current_dir = PathBuf::from("/other");
    let mut left = left;
    left.tabs.push(extra);

    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(left)
        .right(manager(tab_with(listing())))
        .build();

    handle_toggle_hidden(&mut app).await;
    assert!(!app.show_hidden);
    for panel in [&app.panels.left, &app.panels.right] {
        for tab in &panel.tabs {
            assert!(!tab.show_hidden, "every tab follows the global setting");
            assert_eq!(names(tab), vec!["..", "docs", "README.md"]);
        }
    }

    handle_toggle_hidden(&mut app).await;
    assert!(app.show_hidden);
    for panel in [&app.panels.left, &app.panels.right] {
        for tab in &panel.tabs {
            assert!(tab.show_hidden);
            assert_eq!(names(tab).len(), 5);
        }
    }
}

#[tokio::test]
async fn test_toggle_reports_state_in_the_status_line() {
    let mut app = app_with(listing());
    handle_toggle_hidden(&mut app).await;
    let msg = app.active_tab().status_msg.as_ref().unwrap().0.clone();
    assert_eq!(msg, "Hidden files hidden");

    handle_toggle_hidden(&mut app).await;
    let msg = app.active_tab().status_msg.as_ref().unwrap().0.clone();
    assert_eq!(msg, "Hidden files shown");
}

#[test]
fn test_delete_ignores_a_cursor_off_the_visible_listing() {
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    // `.env` is withheld, so it has no row in the visible listing.
    let env = app
        .active_tab()
        .entries
        .iter()
        .position(|e| e.name == ".env")
        .unwrap();
    assert!(!app.active_tab().is_visible(env));

    // A cursor that no longer names a row (as a stale cursor could) must not
    // fall back to acting on an entry that is not on screen.
    app.active_tab_mut().cursor = app.active_tab().visible_count() + 5;

    crate::handlers::popup_delete::handle_init_delete(&mut app, false);
    assert!(
        !app.popups.delete.is_visible,
        "must not offer to delete an entry that is not on screen"
    );
}

#[test]
fn test_copy_ignores_a_cursor_off_the_visible_listing() {
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    app.active_tab_mut().cursor = app.active_tab().visible_count() + 5;

    crate::handlers::popup_copy_move::handle_init_copy(&mut app);
    assert!(
        app.popups.copy_move.source_paths.is_empty(),
        "must not offer to copy an entry that is not on screen"
    );
}

#[test]
fn test_copy_still_works_on_a_visible_entry_with_hidden_hidden() {
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    app.active_tab_mut().cursor = 2; // "README.md", the last visible row

    crate::handlers::popup_copy_move::handle_init_copy(&mut app);
    assert_eq!(app.popups.copy_move.source_paths.len(), 1);
    assert!(app.popups.copy_move.source_paths[0].ends_with("README.md"));
}

#[test]
fn test_toggle_hides_dotfiles_in_a_remote_tab_on_every_platform() {
    // Regression: hidden-ness was decided by `cfg!(windows)` on the client, so
    // from Windows a remote tab withheld nothing at all — the toggle reported
    // "Hidden files hidden" while every remote dotfile stayed listed. A remote
    // listing reports Unix permissions and carries no DOS flags, so the dot
    // convention must apply there regardless of where the app runs.
    let mut tab = remote_tab_with(remote_listing());
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );

    tab.set_show_hidden(false);
    assert_eq!(names(&tab), vec!["..", "docs", "README.md"]);

    tab.set_show_hidden(true);
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );
}

#[tokio::test]
async fn test_toggle_applies_to_a_remote_tab_through_the_app_entry_point() {
    // The same through `handle_toggle_hidden`, which is what the key binding
    // calls: a local and a remote tab in the two panels must both follow.
    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(manager(tab_with(listing())))
        .right(manager(remote_tab_with(remote_listing())))
        .build();

    handle_toggle_hidden(&mut app).await;

    assert!(!app.show_hidden);
    assert_eq!(
        names(app.panels.left.active_tab()),
        vec!["..", "docs", "README.md"]
    );
    assert_eq!(
        names(app.panels.right.active_tab()),
        vec!["..", "docs", "README.md"],
        "a remote tab must honour the same toggle as a local one"
    );
}

/// On Windows a leading dot is not a hidden marker; use the DOS flag instead.
#[cfg(windows)]
fn mark_hidden(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    let rc =
        unsafe { winapi::um::fileapi::SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) };
    assert_eq!(rc, 1, "SetFileAttributesW({:?}) failed", path);
}

#[cfg(not(windows))]
fn mark_hidden(_path: &std::path::Path) {}

#[tokio::test]
async fn test_toggle_refreshes_a_viewer_directory_listing() {
    // The viewer follows the cursor, so its listing is a directory preview. When
    // the hidden setting flips, that preview must reload rather than keep showing
    // the previous rows until the cursor moves.
    let temp = tempfile::tempdir().unwrap();
    let sub = temp.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("visible.txt"), "hi").unwrap();
    std::fs::write(sub.join(".hidden.txt"), "shh").unwrap();
    mark_hidden(&sub.join(".hidden.txt"));

    let mut tab = crate::test_utils::create_test_tab();
    tab.current_dir = temp.path().to_path_buf();
    tab.entries = vec![entry("sub", true, "drwxr-xr-x")];
    tab.cursor = 0;
    tab.recompute_visible_indices();

    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(manager(tab.clone()))
        .right(manager(tab))
        .build();
    app.show_hidden = true;
    app.apply_show_hidden();
    app.file_viewer.is_visible = true;
    crate::handlers::navigation::update_viewer_content(&mut app).await;
    assert!(
        app.file_viewer
            .text
            .content
            .iter()
            .any(|l| l == ".hidden.txt"),
        "precondition: the hidden entry is listed while hidden files are shown"
    );

    handle_toggle_hidden(&mut app).await;
    assert!(!app.show_hidden);
    assert!(
        !app.file_viewer
            .text
            .content
            .iter()
            .any(|l| l == ".hidden.txt"),
        "the listing must follow the toggle, got {:?}",
        app.file_viewer.text.content
    );

    handle_toggle_hidden(&mut app).await;
    assert!(
        app.file_viewer
            .text
            .content
            .iter()
            .any(|l| l == ".hidden.txt"),
        "showing hidden again must bring the entry back"
    );
}

#[test]
fn test_local_windows_tab_still_ignores_dot_prefixed_names() {
    // The mirror image of the remote case: a local Windows filesystem has no
    // dotfile convention, so an unflagged `.env` stays listed there.
    if !cfg!(windows) {
        return;
    }
    let mut tab = tab_with(vec![
        entry("..", true, ""),
        entry(".env", false, "------"),
        entry("README.md", false, "------"),
    ]);
    tab.set_show_hidden(false);
    assert_eq!(names(&tab), vec!["..", ".env", "README.md"]);
}
