use crate::app_state::tabs::SortColumn;
use crate::app_state::tabs::Tab;
use crate::theme::ThemePalette;
use crate::ui::ui_utils::{
    TabScrollbarContext, draw_tab_scrollbar, lighten_red, panel_bg_color, replace_home_with_tilde,
    truncate_path_str, truncate_path_with_ellipsis,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

use super::entry::sort_indicator;

/// Build the panel block with title and borders
pub(super) fn build_panel_block(
    area: Rect,
    palette: &ThemePalette,
    active: bool,
    borders: bool,
    is_root: bool,
    panel: &Tab,
) -> Block<'static> {
    let border_color = if is_root && active {
        Color::Rgb(palette.red.r, palette.red.g, palette.red.b)
    } else if is_root && !active {
        let light_red = lighten_red(palette.red);
        Color::Rgb(light_red.r, light_red.g, light_red.b)
    } else if active {
        Color::Rgb(palette.border.r, palette.border.g, palette.border.b)
    } else {
        Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
    };

    let panel_bg = panel_bg_color(palette, active, is_root, borders);

    let prefix = panel.provider.display_prefix();

    let display_path = if panel.provider.is_local() {
        None
    } else {
        Some(panel.provider.display_path(&panel.current_dir))
    };

    let tilde_path = match &display_path {
        Some(p) => p.clone(),
        None => replace_home_with_tilde(&panel.current_dir),
    };
    let full_title = if prefix.is_empty() {
        format!(" {tilde_path} ")
    } else {
        format!(" {prefix}:{tilde_path} ")
    };
    let title_width = area.width.saturating_sub(4) as usize;
    let panel_title = if full_title.len() > title_width {
        match &display_path {
            Some(p) => format!(" {prefix}:{} ", truncate_path_str(p, title_width)),
            None => truncate_path_with_ellipsis(&panel.current_dir, title_width),
        }
    } else {
        full_title
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title)
        .border_style(Style::default().fg(border_color).bg(panel_bg))
        .style(Style::default().bg(panel_bg));

    if borders {
        block.border_type(ratatui::widgets::BorderType::Rounded)
    } else {
        block.border_set(ratatui::symbols::border::EMPTY)
    }
}

/// Draw selection markers on the left side
pub(super) fn draw_selection_markers(
    frame: &mut ratatui::Frame,
    area: Rect,
    panel: &Tab,
    visible_rows: usize,
    palette: &ThemePalette,
) {
    let yellow_color = Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b);

    // Row-to-entry mapping is shared with the mouse hit-testing code.
    for (row, idx) in (0..visible_rows)
        .map_while(|row| panel.visible_row_to_entry_index(row))
        .enumerate()
    {
        let entry = &panel.entries[idx];
        let row_offset = u16::try_from(row).unwrap_or(u16::MAX);
        let row_y = area.y + 2 + row_offset; // +2 for border and header

        if row_y >= area.y + area.height - 1 {
            break;
        }

        if entry.selected {
            let marker = Span::styled("▊", Style::default().fg(yellow_color));
            frame.render_widget(
                Line::from(marker),
                Rect {
                    x: area.x,
                    y: row_y,
                    width: 1,
                    height: 1,
                },
            );
        }
    }
}

/// Draw vertical scrollbar
pub(super) fn draw_scrollbar(
    frame: &mut ratatui::Frame,
    area: Rect,
    panel: &Tab,
    visible_rows: usize,
    ctx: &TabScrollbarContext,
) {
    let total_entries = panel.visible_count();

    let scroll_area = Rect {
        x: area.x + area.width - 1,
        y: area.y + 2, // +2 for border and header
        width: 1,
        height: u16::try_from(visible_rows).unwrap_or(u16::MAX),
    };

    draw_tab_scrollbar(
        frame,
        scroll_area,
        total_entries,
        visible_rows,
        panel.cursor,
        ctx,
        false,
    );
}

pub(super) fn build_header_row(
    panel: &Tab,
    size_header: &str,
    unix_attributes: bool,
) -> [String; 4] {
    let name_indicator = sort_indicator(SortColumn::Name, panel.sort.column, panel.sort.direction);
    let ext_indicator = sort_indicator(
        SortColumn::Extension,
        panel.sort.column,
        panel.sort.direction,
    );
    let name_header = if ext_indicator.is_empty() {
        format!("Name{name_indicator}")
    } else {
        format!("Name{ext_indicator}")
    };
    let modified_header = format!(
        "Modified{}",
        sort_indicator(SortColumn::Date, panel.sort.column, panel.sort.direction)
    );
    // Only the short Windows attribute flags fit "Attrib"; Unix-style
    // permissions need the full "Attributes" label.
    let attributes_header = if unix_attributes {
        "Attributes".to_string()
    } else {
        "Attrib".to_string()
    };
    [
        name_header,
        size_header.to_string(),
        modified_header,
        attributes_header,
    ]
}
