use super::*;

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
