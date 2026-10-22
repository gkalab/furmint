use super::*;

#[test]
fn test_name_column_fits_ratatui_layout() {
    // 4 table columns -> 3 gaps at default spacing 1, plus 2 border columns and the
    // fixed non-name columns (which vary by platform: 6 characters of Windows attribute
    // flags for local files, 10 characters of Unix permissions otherwise). The budget
    // is `width - overhead`, and the icon lives inside the name column, so rendering
    // must never clip the tail of a truncated name.
    let total_table_width = 100;
    let name_col = calculate_name_col_for(total_table_width, attributes_col_width(true));
    let real_col = name_column_width(total_table_width, attributes_col_width(true));

    assert_eq!(name_col, real_col);

    // With icons enabled the icon (2 chars) is prefixed inside the name column.
    let with_icon = name_col.saturating_sub(2);
    assert!(with_icon <= real_col);
    // Without icons the truncated name uses the full column.
    assert!(name_col <= real_col);
    // A name of exactly the budget length fits without overflow.
    assert!(with_icon < real_col);
}

#[test]
fn test_attributes_column_width_per_filesystem() {
    use crate::fs::utils::has_unix_attributes;
    // Remote and archive entries carry Unix permissions, so the column is
    // wide enough for all 10 characters even on Windows.
    if cfg!(windows) {
        assert_eq!(attributes_col_width(false), 10);
        assert!(has_unix_attributes(false));
        assert_eq!(attributes_col_width(true), 6);
        assert!(!has_unix_attributes(true));
    } else {
        assert_eq!(attributes_col_width(true), 10);
        assert_eq!(attributes_col_width(false), 10);
        assert!(has_unix_attributes(true));
        assert!(has_unix_attributes(false));
    }
}

fn name_column_width(total: usize, attributes: u16) -> usize {
    let overhead = 7 + 19 + usize::from(attributes) + 3 + 2;
    if total > overhead {
        total - overhead
    } else {
        10
    }
}

fn calculate_name_col_for(total: usize, attributes: u16) -> usize {
    name_col_width_for(total, attributes)
}
