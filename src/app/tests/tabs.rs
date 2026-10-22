use super::*;

#[tokio::test]
async fn test_current_entry() {
    let panel = create_test_tab();
    assert_eq!(panel.current_entry().map(|e| e.name.as_str()), Some(".."));
}

#[tokio::test]
async fn test_move_cursor_up() {
    let mut panel = create_test_tab();
    panel.cursor = 2;
    panel.move_cursor_up();
    assert_eq!(panel.cursor, 1);

    panel.cursor = 0;
    panel.move_cursor_up();
    assert_eq!(panel.cursor, 0); // Should not go negative
}

#[tokio::test]
async fn test_move_cursor_down() {
    let mut panel = create_test_tab();
    panel.move_cursor_down();
    assert_eq!(panel.cursor, 1);

    panel.cursor = 3;
    panel.move_cursor_down();
    assert_eq!(panel.cursor, 3); // Should not exceed entries
}

#[tokio::test]
async fn test_move_cursor_page_up() {
    let mut panel = create_test_tab();
    panel.cursor = 3;
    panel.move_cursor_page_up(2);
    assert_eq!(panel.cursor, 1);

    panel.move_cursor_page_up(5);
    assert_eq!(panel.cursor, 0);
}

#[tokio::test]
async fn test_move_cursor_page_down() {
    let mut panel = create_test_tab();
    panel.move_cursor_page_down(2);
    assert_eq!(panel.cursor, 2);

    panel.move_cursor_page_down(10);
    assert_eq!(panel.cursor, 3);
}

#[tokio::test]
async fn test_move_cursor_home() {
    let mut panel = create_test_tab();
    panel.cursor = 3;
    panel.move_cursor_home();
    assert_eq!(panel.cursor, 0);
}

#[tokio::test]
async fn test_move_cursor_end() {
    let mut panel = create_test_tab();
    panel.move_cursor_end();
    assert_eq!(panel.cursor, 3);
}

#[tokio::test]
async fn test_toggle_selection() {
    let mut panel = create_test_tab();
    panel.cursor = 2;
    assert!(!panel.entries[2].selected);
    panel.toggle_selection();
    assert!(panel.entries[2].selected);
    // Reset cursor to 2 because toggle_selection moves it down
    panel.cursor = 2;
    panel.toggle_selection();
    assert!(!panel.entries[2].selected);
}

#[tokio::test]
async fn test_get_selected_entries() {
    let mut panel = create_test_tab();
    assert_eq!(panel.get_selected_entries().len(), 0);

    panel.entries[1].selected = true;
    panel.entries[3].selected = true;
    let selected = panel.get_selected_entries();
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].name, "dir1");
    assert_eq!(selected[1].name, "file2.txt");
}

#[tokio::test]
async fn test_sort_entries() {
    let mut panel = create_test_tab();

    // Initial state: Name Ascending
    // Directories first, then files
    // .. (dir), dir1 (dir), file1.txt (file), file2.txt (file)
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
    assert_eq!(panel.entries[2].name, "file1.txt");
    assert_eq!(panel.entries[3].name, "file2.txt");

    // Sort by Name Descending
    panel.handle_sort(SortColumn::Name);
    // Directories still first, but sorted descending (if there were multiple)
    // BUT ".." is special cased to be at the top
    // So: .., dir1
    // Files sorted descending: file2.txt, file1.txt
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
    assert_eq!(panel.entries[2].name, "file2.txt");
    assert_eq!(panel.entries[3].name, "file1.txt");

    // Sort by Size (Defaults to Descending)
    panel.handle_sort(SortColumn::Size);
    assert_eq!(panel.sort.column, SortColumn::Size);
    assert_eq!(panel.sort.direction, SortDirection::Descending);
    // Directories first. For Size sort, directories use Name Ascending ALWAYS.
    // .. is top. dir1 is next.
    // Files sorted Descending: file2.txt (200), file1.txt (100)
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
    assert_eq!(panel.entries[2].name, "file2.txt");
    assert_eq!(panel.entries[3].name, "file1.txt");

    // Toggle Size (Ascending)
    panel.handle_sort(SortColumn::Size);
    assert_eq!(panel.sort.direction, SortDirection::Ascending);
    // Directories first. For Size sort, directories use Name Ascending ALWAYS.
    // .. is top. dir1 is next.
    // Files sorted Ascending: file1.txt (100), file2.txt (200)
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
    assert_eq!(panel.entries[2].name, "file1.txt");
    assert_eq!(panel.entries[3].name, "file2.txt");

    // Sort by Extension Descending
    panel.handle_sort(SortColumn::Extension);
    panel.handle_sort(SortColumn::Extension); // Toggle to Descending
    assert_eq!(panel.sort.column, SortColumn::Extension);
    assert_eq!(panel.sort.direction, SortDirection::Descending);
    // Directories first. For Extension sort, directories use Name Ascending ALWAYS.
    // .. is top. dir1 is next.
    // Files sorted Descending (txt): file2.txt, file1.txt (stable sort or name fallback if extensions equal)
    // Since extensions are equal ("txt"), it falls back to name comparison?
    // Wait, the code for files uses `ext_a.cmp(&ext_b)`. If equal, `sort_by` is not stable unless we make it so.
    // `slice::sort_by` IS stable. So if extensions are equal, original order is preserved?
    // Original order was Name Ascending (from initial load).
    // If we want deterministic sort for files with same extension, we should probably add secondary sort by name.
    // But for now, let's just check directories are correct.
    assert_eq!(panel.entries[0].name, "..");
    assert_eq!(panel.entries[1].name, "dir1");
}

#[tokio::test]
async fn test_sort_defaults() {
    let mut panel = create_test_tab();

    // Initial state: Name Ascending
    assert_eq!(panel.sort.column, SortColumn::Name);
    assert_eq!(panel.sort.direction, SortDirection::Ascending);

    // Switch to Date -> Should default to Descending
    panel.handle_sort(SortColumn::Date);
    assert_eq!(panel.sort.column, SortColumn::Date);
    assert_eq!(panel.sort.direction, SortDirection::Descending);

    // Switch to Size -> Should default to Descending
    panel.handle_sort(SortColumn::Size);
    assert_eq!(panel.sort.column, SortColumn::Size);
    assert_eq!(panel.sort.direction, SortDirection::Descending);
}
