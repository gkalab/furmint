use super::*;

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
