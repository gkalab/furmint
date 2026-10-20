use crate::app_state::tabs::SortColumn;
use crate::app_state::tabs::Tab;
use crate::fs::utils::format_size;
use ratatui::prelude::*;

use super::entry::sort_indicator;

/// Holds calculated column widths for the panel table
pub(super) struct ColumnWidths {
    pub(super) name: usize,         // Available width for name column
    pub(super) size_header: String, // Formatted size header with sort indicator
    pub(super) attributes: u16,     // Width of the attributes column
    /// True when the attributes column holds Unix-style permissions
    pub(super) unix_attributes: bool,
}

/// Width of the attributes column when the entries carry Unix-style
/// permissions (`drwxr-xr-x`).
const UNIX_ATTRIBUTES_COL_WIDTH: u16 = 10;
/// Width of the attributes column for local files on Windows, where the flags
/// are a type char plus 5 attributes (`d---r-`).
const WINDOWS_FLAG_COL_WIDTH: u16 = 6;

/// Width of the attributes column for the given filesystem.
#[must_use]
pub(crate) fn attributes_col_width(local: bool) -> u16 {
    if crate::fs::utils::has_unix_attributes(local) {
        UNIX_ATTRIBUTES_COL_WIDTH
    } else {
        WINDOWS_FLAG_COL_WIDTH
    }
}

/// Available width for the name column, matching what ratatui's `Table` allocates.
///
/// The fixed non-name columns are Size(7) + Modified(19) + the attributes column.
/// On top of those, ratatui reserves the block borders (2 columns) and the default
/// `column_spacing` (1) between the 4 table columns (3 gaps).
pub(super) fn name_col_width_for(total_table_width: usize, attributes_width: u16) -> usize {
    let other_cols = 7 + 19 + usize::from(attributes_width);
    let overhead = other_cols + 3 + 2; // 3 gaps between 4 columns, 2 for borders
    if total_table_width > overhead {
        total_table_width - overhead
    } else {
        10 // minimum width for name
    }
}

/// Calculate column widths and headers for the panel
pub(super) fn calculate_column_widths(panel: &Tab, area: Rect) -> ColumnWidths {
    // Compute max width for Size column
    let size_width = panel
        .entries
        .iter()
        .map(|e| format_size(e.size, e.is_dir, e.is_symlink).len())
        .max()
        .unwrap_or(4);

    let size_indicator = sort_indicator(SortColumn::Size, panel.sort.column, panel.sort.direction);
    let size_header_with_indicator = format!("Size{size_indicator}");
    let size_header = format!(
        "{:>width$}",
        size_header_with_indicator,
        width = size_width.max(size_header_with_indicator.len())
    );
    let _ = size_width; // size_width is used in size_header calculation

    let local = panel.provider.is_local();
    let attributes = attributes_col_width(local);

    // Calculate available width for name column.
    let name_col_width = name_col_width_for(area.width as usize, attributes);

    ColumnWidths {
        name: name_col_width,
        size_header,
        attributes,
        unix_attributes: crate::fs::utils::has_unix_attributes(local),
    }
}
