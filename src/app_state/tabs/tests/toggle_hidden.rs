//! Tests for the global show/hide of hidden files and directories.

use crate::app::AppState;
use crate::app_state::tabs::{Tab, TabManager};
use crate::fs::utils::FileEntry;
use crate::handlers::toggle_hidden::handle_toggle_hidden;
use std::path::PathBuf;

/// A Unix-style listing: `..`, a dot-directory, a plain directory, a dotfile and
/// a plain file. Sorts in the order given.
fn listing() -> Vec<FileEntry> {
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
    tab.cursor = 1; // "alpha.rs", the only filtered-in file

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
    assert!(tab.is_visible(tab.cursor));
}

#[test]
fn test_wheel_stepping_skips_hidden_entries() {
    // The mouse wheel steps through the visible listing, so it must never land
    // on a withheld entry.
    let mut tab = tab_with(listing());
    tab.set_show_hidden(false);
    tab.cursor = 2; // "docs", the first visible entry after ".."
    for _ in 0..3 {
        tab.move_cursor_down_filtered();
        assert!(
            tab.is_visible(tab.cursor),
            "cursor landed on {:?}",
            tab.entries[tab.cursor].name
        );
    }
    assert_eq!(tab.entries[tab.cursor].name, "README.md");
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
    // The platform difference (leading dot on Unix, DOS attribute on Windows)
    // must stay in `fs::utils::is_hidden` rather than being reimplemented here.
    let entries = vec![
        entry("..", true, ""),
        entry(".dotfile", false, "-rw-r--r--"),
        entry("flagged", false, "-a-h"),
        entry("plain.txt", false, "-rw-r--r--"),
    ];
    let mut tab = tab_with(entries);
    tab.set_show_hidden(false);

    for i in 0..tab.entries.len() {
        let e = &tab.entries[i];
        assert_eq!(
            tab.is_visible(i),
            e.name == ".." || !crate::fs::utils::is_hidden(&e.name, &e.attributes),
            "visibility of {:?} disagrees with is_hidden",
            e.name
        );
    }
}

#[test]
fn test_cursor_moves_onto_a_visible_entry() {
    let mut tab = tab_with(listing());
    tab.cursor = 3; // ".env"
    tab.set_show_hidden(false);
    assert!(tab.is_visible(tab.cursor));
    assert_eq!(tab.entries[tab.cursor].name, "docs");
}

#[test]
fn test_cursor_pinned_to_zero_when_nothing_is_visible() {
    let mut tab = tab_with(vec![
        entry("..", true, ""),
        entry(".env", false, "-rw-r--r--"),
    ]);
    tab.cursor = 1;
    tab.set_show_hidden(false);
    // Only ".." survives; an empty set must not leave the cursor on ".env".
    assert_eq!(names(&tab), vec![".."]);
    assert_eq!(tab.cursor, 0);
    assert_eq!(tab.entries[tab.cursor].name, "..");
}

#[test]
fn test_cursor_pinned_when_filter_matches_nothing() {
    // ".." is a directory and always survives, so use a files-only listing to
    // reach a genuinely empty visible set.
    let mut tab = tab_with(vec![
        entry("a.txt", false, "-rw-r--r--"),
        entry("b.txt", false, "-rw-r--r--"),
    ]);
    tab.cursor = 1; // "b.txt"
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

#[test]
fn test_handle_toggle_hidden_applies_to_every_tab_in_both_panels() {
    let left = manager(tab_with(listing()));
    let mut extra = tab_with(listing());
    extra.current_dir = PathBuf::from("/other");
    let mut left = left;
    left.tabs.push(extra);

    let mut app = crate::test_utils::TestAppBuilder::new()
        .left(left)
        .right(manager(tab_with(listing())))
        .build();

    handle_toggle_hidden(&mut app);
    assert!(!app.show_hidden);
    for panel in [&app.panels.left, &app.panels.right] {
        for tab in &panel.tabs {
            assert!(!tab.show_hidden, "every tab follows the global setting");
            assert_eq!(names(tab), vec!["..", "docs", "README.md"]);
        }
    }

    handle_toggle_hidden(&mut app);
    assert!(app.show_hidden);
    for panel in [&app.panels.left, &app.panels.right] {
        for tab in &panel.tabs {
            assert!(tab.show_hidden);
            assert_eq!(names(tab).len(), 5);
        }
    }
}

#[test]
fn test_toggle_reports_state_in_the_status_line() {
    let mut app = app_with(listing());
    handle_toggle_hidden(&mut app);
    let msg = app.active_tab().status_msg.as_ref().unwrap().0.clone();
    assert_eq!(msg, "Hidden files hidden");

    handle_toggle_hidden(&mut app);
    let msg = app.active_tab().status_msg.as_ref().unwrap().0.clone();
    assert_eq!(msg, "Hidden files shown");
}

#[test]
fn test_delete_ignores_a_cursor_sitting_on_a_hidden_entry() {
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    // Force the cursor onto the hidden entry, as a stale cursor would be.
    app.active_tab_mut().cursor = 3;
    assert_eq!(app.active_tab().entries[3].name, ".env");

    crate::handlers::popup_delete::handle_init_delete(&mut app, false);
    assert!(
        !app.popups.delete.is_visible,
        "must not offer to delete an entry that is not on screen"
    );
}

#[test]
fn test_copy_ignores_a_cursor_sitting_on_a_hidden_entry() {
    let mut app = app_with(listing());
    app.set_show_hidden(false);
    app.active_tab_mut().cursor = 3;

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
    app.active_tab_mut().cursor = 4; // "README.md"

    crate::handlers::popup_copy_move::handle_init_copy(&mut app);
    assert_eq!(app.popups.copy_move.source_paths.len(), 1);
    assert!(app.popups.copy_move.source_paths[0].ends_with("README.md"));
}
