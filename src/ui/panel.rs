//! Rendering of a file panel.
//!
//! [`layout`] computes the column widths, [`entry`] draws a single listing row,
//! [`chrome`] draws the block, selection markers and scrollbar, and [`status`]
//! renders the status line.

mod chrome;
mod entry;
mod layout;
mod status;

use crate::app_state::tabs::Tab;
use crate::theme::ThemePalette;
use crate::ui::ui_utils::{TabScrollbarContext, is_root_user};
use ratatui::prelude::*;
use ratatui::widgets::{Row, Table, TableState};

use self::chrome::{build_header_row, build_panel_block, draw_scrollbar, draw_selection_markers};
use self::entry::{EntryRowContext, render_entry_row};
use self::layout::calculate_column_widths;

pub(crate) use self::entry::{entry_foreground, entry_icon_foreground};
pub(crate) use self::layout::attributes_col_width;
pub use self::status::{PanelStatusContext, draw_panel_status};

/// Draws the main file panel with entries and sorting headers.
pub fn draw_panel(
    f: &mut ratatui::Frame,
    panel: &mut Tab,
    active: bool,
    area: Rect,
    palette: &ThemePalette,
    borders: bool,
    icons_enabled: bool,
) {
    // Calculate visible rows for scrolling logic
    let visible_rows = area.height.saturating_sub(3) as usize; // -2 for borders, -1 for header
    panel.scroll_to_cursor(visible_rows);

    let col_widths = calculate_column_widths(panel, area);
    let header = build_header_row(panel, &col_widths.size_header, col_widths.unix_attributes);

    // Build entry rows
    let ctx = EntryRowContext {
        palette,
        icons_enabled,
        current_dir: &panel.current_dir,
        name_col_width: col_widths.name,
        dir_sizes: &panel.dir_sizes,
        unix_attributes: col_widths.unix_attributes,
    };

    // Determine which entries to render. Both rendering and mouse hit-testing go
    // through `visible_row_to_entry_index`, so they cannot disagree.
    let rows: Vec<Row> = (0..visible_rows)
        .map_while(|row| panel.visible_row_to_entry_index(row))
        .map(|idx| {
            let entry = &panel.entries[idx];
            render_entry_row(entry, idx, &ctx, &panel.search.highlights)
        })
        .collect();

    // Build and render the table
    let is_root = is_root_user(panel);
    let block = build_panel_block(area, palette, active, borders, is_root, panel);

    let widths = [
        Constraint::Min(10),                       // Name: dynamic, at least 10
        Constraint::Length(7),                     // Size: always 7 (right-aligned)
        Constraint::Length(19),                    // Modified: always 19
        Constraint::Length(col_widths.attributes), // Attributes
    ];

    let panel_selection_background = if active {
        Color::Rgb(palette.surface0.r, palette.surface0.g, palette.surface0.b)
    } else {
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    };

    let table = Table::new(rows, widths)
        .header(Row::new(header).style(Style::default().fg(Color::Rgb(
            palette.yellow.r,
            palette.yellow.g,
            palette.yellow.b,
        ))))
        .block(block);

    let table = if active {
        table.row_highlight_style(Style::default().bg(panel_selection_background))
    } else {
        table
    };

    // The cursor is already a visible row position; shift it into the rendered
    // window.
    let highlighted_row = Some(panel.cursor.saturating_sub(panel.scroll_offset));

    f.render_stateful_widget(
        table,
        area,
        &mut TableState::default().with_selected(highlighted_row),
    );

    draw_selection_markers(f, area, panel, visible_rows, palette);

    draw_scrollbar(
        f,
        area,
        panel,
        visible_rows,
        &TabScrollbarContext {
            palette,
            borders,
            is_root,
            active,
        },
    );
}

#[cfg(test)]
mod tests;
