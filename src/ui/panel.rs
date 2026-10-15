use crate::app_state::tabs::Tab;
use crate::app_state::tabs::{SortColumn, SortDirection};
use crate::fs::utils::{FileEntry, format_modified, format_size};
use crate::theme::{Rgb, ThemePalette};
use crate::ui::ui_utils::{
    TabScrollbarContext, draw_tab_scrollbar, is_root_user, lighten_red, panel_bg_color,
    truncate_middle_with_ellipsis, truncate_path_str,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use std::collections::HashMap;
use std::path::Path;

/// Holds calculated column widths for the panel table
struct ColumnWidths {
    name: usize,         // Available width for name column
    size_header: String, // Formatted size header with sort indicator
    attributes: u16,     // Width of the attributes column
    /// True when the attributes column holds Unix-style permissions
    unix_attributes: bool,
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

/// Context for rendering an entry row
struct EntryRowContext<'a> {
    palette: &'a ThemePalette,
    icons_enabled: bool,
    current_dir: &'a Path,
    name_col_width: usize,
    /// Cached directory sizes: path -> size in bytes
    dir_sizes: &'a std::collections::HashMap<std::path::PathBuf, u64>,
    /// True when `entry.attributes` holds Unix-style permissions
    /// (`drwxr-xr-x`) rather than the short Windows flag format.
    unix_attributes: bool,
}

fn sort_indicator(
    column: SortColumn,
    current_column: SortColumn,
    direction: SortDirection,
) -> &'static str {
    if column == current_column {
        match direction {
            SortDirection::Ascending => "▴",
            SortDirection::Descending => "▾",
        }
    } else {
        ""
    }
}

/// Available width for the name column, matching what ratatui's `Table` allocates.
///
/// The fixed non-name columns are Size(7) + Modified(19) + the attributes column.
/// On top of those, ratatui reserves the block borders (2 columns) and the default
/// `column_spacing` (1) between the 4 table columns (3 gaps).
fn name_col_width_for(total_table_width: usize, attributes_width: u16) -> usize {
    let other_cols = 7 + 19 + usize::from(attributes_width);
    let overhead = other_cols + 3 + 2; // 3 gaps between 4 columns, 2 for borders
    if total_table_width > overhead {
        total_table_width - overhead
    } else {
        10 // minimum width for name
    }
}

/// Calculate column widths and headers for the panel
fn calculate_column_widths(panel: &Tab, area: Rect) -> ColumnWidths {
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

/// Get style for a file entry based on its type.
///
/// Symlinks keep their own cyan regardless of the target. Everything else that
/// is hidden (a leading dot on a Unix filesystem, `FILE_ATTRIBUTE_HIDDEN` on a
/// local Windows one) is de-emphasized with the muted `subtext` color, whether
/// it is a file or a directory. `unix_attributes` is
/// [`crate::fs::utils::has_unix_attributes`] for the browsed filesystem, so a
/// remote tab mutes the remote host's dotfiles from any platform.
fn style_for_entry(
    entry: &FileEntry,
    current_dir: &Path,
    palette: &ThemePalette,
    unix_attributes: bool,
) -> Style {
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
    if entry.is_symlink {
        return Style::default().fg(rgb(palette.cyan));
    }
    if crate::fs::utils::is_hidden(&entry.name, &entry.attributes, unix_attributes) {
        return Style::default().fg(rgb(palette.subtext));
    }
    if entry.is_dir {
        return Style::default().fg(rgb(palette.blue));
    }
    if crate::fs::utils::is_executable(&current_dir.join(&entry.name), entry) {
        return Style::default().fg(rgb(palette.green));
    }
    Style::default().fg(rgb(palette.text))
}

/// Style of the icon drawn in front of an entry name.
///
/// Only hidden directories deviate from their name color: their icon keeps the
/// standard directory blue so the row still reads as a directory, while the
/// muted name carries the "hidden" signal. Every other entry matches its name.
fn style_for_icon(
    entry: &FileEntry,
    palette: &ThemePalette,
    name_style: Style,
    unix_attributes: bool,
) -> Style {
    if entry.is_dir
        && !entry.is_symlink
        && crate::fs::utils::is_hidden(&entry.name, &entry.attributes, unix_attributes)
    {
        return Style::default().fg(Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b));
    }
    name_style
}

/// Style a single character of a Unix attributes string (`drwxrwxrwx`):
/// the `d` matches directories (blue), `l` matches symlinks (cyan), `x` matches
/// executables (green), `w` uses the normal text color, and `r`/`-` use the
/// muted overlay color (same as the inactive tab title). Local files on Windows
/// use a different attribute-flag format, so their attributes keep the plain
/// text color.
fn attributes_cell(
    attrs: &str,
    palette: &ThemePalette,
    text_fg: Color,
    unix_style: bool,
) -> Cell<'static> {
    let default_style = Style::default().fg(text_fg);
    if unix_style {
        let spans: Vec<Span> = attrs
            .chars()
            .map(|c| Span::styled(c.to_string(), attributes_char_style(c, palette, text_fg)))
            .collect();
        Cell::from(Line::from(spans))
    } else {
        Cell::from(attrs.to_string()).style(default_style)
    }
}

/// Style of a single character of a Unix attributes string.
fn attributes_char_style(c: char, palette: &ThemePalette, text_fg: Color) -> Style {
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
    let color = match c {
        'd' => rgb(palette.blue),
        'l' => rgb(palette.cyan),
        'x' => rgb(palette.green),
        'r' | '-' => rgb(palette.overlay0),
        _ => text_fg,
    };
    Style::default().fg(color)
}

/// Nerd-font icon for an entry, or an empty string when the filesystem does not
/// provide one.
fn entry_icon(entry: &FileEntry, current_dir: &Path) -> &'static str {
    crate::icons::get_icon(
        &entry.name,
        entry.is_dir,
        entry.is_symlink,
        crate::fs::utils::is_executable(&current_dir.join(&entry.name), entry),
    )
}

/// Render a single entry row
fn render_entry_row<'a>(
    entry: &'a FileEntry,
    idx: usize,
    ctx: &'a EntryRowContext<'a>,
    search_highlights: &'a HashMap<usize, Vec<usize>>,
) -> Row<'a> {
    let text_fg = Color::Rgb(ctx.palette.text.r, ctx.palette.text.g, ctx.palette.text.b);
    let name_style = style_for_entry(entry, ctx.current_dir, ctx.palette, ctx.unix_attributes);
    let icon_style = style_for_icon(entry, ctx.palette, name_style, ctx.unix_attributes);

    // Account for ratatui border: subtract 2 from available width
    // Also account for icon width (2 chars: icon + space) if icons are enabled
    let icon_width = if ctx.icons_enabled { 2 } else { 0 };
    let visible_name_width = if ctx.name_col_width > icon_width {
        ctx.name_col_width - icon_width
    } else {
        1
    };

    let name_cell = if let Some(matches) = search_highlights.get(&idx) {
        let yellow = Color::Rgb(
            ctx.palette.yellow.r,
            ctx.palette.yellow.g,
            ctx.palette.yellow.b,
        );
        let highlight_style = Style::default().fg(yellow).add_modifier(Modifier::BOLD);

        let name_chars: Vec<char> = entry.name.chars().collect();
        let name_len = name_chars.len();
        let mut spans = Vec::new();

        // Add icon if enabled
        if ctx.icons_enabled {
            let icon = entry_icon(entry, ctx.current_dir);
            spans.push(Span::styled(format!("{icon} "), icon_style));
        }

        if name_len <= visible_name_width {
            for (i, c) in name_chars.iter().enumerate() {
                if matches.contains(&i) {
                    spans.push(Span::styled(c.to_string(), highlight_style));
                } else {
                    spans.push(Span::raw(c.to_string()));
                }
            }
        } else {
            // Truncate logic
            let ellipsis = "…";
            let ellipsis_len = 1;
            let keep = visible_name_width.saturating_sub(ellipsis_len);
            let left = keep / 2;
            let right = keep - left;

            // Left part
            for (i, c) in name_chars.iter().take(left).enumerate() {
                if matches.contains(&i) {
                    spans.push(Span::styled(c.to_string(), highlight_style));
                } else {
                    spans.push(Span::raw(c.to_string()));
                }
            }
            // Ellipsis
            spans.push(Span::raw(ellipsis));
            // Right part
            let start_right = name_len.saturating_sub(right);
            for (i, c) in name_chars.iter().skip(start_right).enumerate() {
                let original_idx = start_right + i;
                if matches.contains(&original_idx) {
                    spans.push(Span::styled(c.to_string(), highlight_style));
                } else {
                    spans.push(Span::raw(c.to_string()));
                }
            }
        }
        Cell::from(Line::from(spans)).style(name_style)
    } else {
        let truncated_name = truncate_middle_with_ellipsis(&entry.name, visible_name_width);
        if ctx.icons_enabled {
            let icon = entry_icon(entry, ctx.current_dir);
            Cell::from(Line::from(vec![
                Span::styled(format!("{icon} "), icon_style),
                Span::raw(truncated_name),
            ]))
            .style(name_style)
        } else {
            Cell::from(truncated_name).style(name_style)
        }
    };

    // Check if we have a cached size for this directory
    let size_to_display = if entry.is_dir && entry.name != ".." {
        let full_path = ctx.current_dir.join(&entry.name);
        ctx.dir_sizes.get(&full_path).copied()
    } else {
        None
    };

    Row::new(vec![
        name_cell,
        Cell::from(format_size(
            size_to_display.or(entry.size),
            entry.is_dir && size_to_display.is_none(),
            entry.is_symlink,
        ))
        .style(Style::default().fg(text_fg)),
        Cell::from(format_modified(entry.modified)).style(Style::default().fg(text_fg)),
        attributes_cell(
            entry.attributes.as_str(),
            ctx.palette,
            text_fg,
            ctx.unix_attributes,
        ),
    ])
}

/// Build the panel block with title and borders
fn build_panel_block(
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
        None => crate::ui::ui_utils::replace_home_with_tilde(&panel.current_dir),
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
            None => {
                crate::ui::ui_utils::truncate_path_with_ellipsis(&panel.current_dir, title_width)
            }
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
fn draw_selection_markers(
    frame: &mut ratatui::Frame,
    area: Rect,
    panel: &Tab,
    visible_rows: usize,
    palette: &ThemePalette,
) {
    let yellow_color = Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b);

    // Determine which indices to render (respecting file filter)
    let render_indices: Box<dyn Iterator<Item = usize>> = if panel.has_visibility_restriction() {
        Box::new(panel.visible_indices.iter().copied())
    } else {
        Box::new(0..panel.entries.len())
    };

    for (row, idx) in render_indices
        .skip(panel.scroll_offset)
        .take(visible_rows)
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
fn draw_scrollbar(
    frame: &mut ratatui::Frame,
    area: Rect,
    panel: &Tab,
    visible_rows: usize,
    ctx: &TabScrollbarContext,
) {
    let total_entries = if panel.has_visibility_restriction() {
        panel.visible_count()
    } else {
        panel.entries.len()
    };

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
        panel.cursor_visible_pos().unwrap_or(panel.cursor),
        ctx,
        false,
    );
}

fn build_header_row(panel: &Tab, size_header: &str, unix_attributes: bool) -> [String; 4] {
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

    // Determine which entries to render (respecting file filter)
    let render_indices: Vec<usize> = if panel.has_visibility_restriction() {
        panel.visible_indices[panel.scroll_offset..]
            .iter()
            .take(visible_rows)
            .copied()
            .collect()
    } else {
        (panel.scroll_offset..)
            .take(visible_rows)
            .take(panel.entries.len())
            .collect()
    };

    let rows: Vec<Row> = render_indices
        .iter()
        .map(|&idx| {
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

    // Map cursor to the visible row position
    let highlighted_row = if panel.has_visibility_restriction() {
        panel
            .cursor_visible_pos()
            .map(|pos| pos.saturating_sub(panel.scroll_offset))
    } else {
        Some(panel.cursor.saturating_sub(panel.scroll_offset))
    };

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

/// Context for rendering panel status bar
pub struct PanelStatusContext<'a> {
    pub palette: &'a ThemePalette,
    pub active: bool,
    pub borders: bool,
    pub task_manager: &'a crate::tasks::TaskManager,
    pub side: crate::app_state::tabs::PanelSide,
}

/// Render the status bar for a panel, showing file counts, selection, and errors.
pub fn draw_panel_status(
    f: &mut ratatui::Frame,
    panel: &Tab,
    area: Rect,
    ctx: &PanelStatusContext,
) {
    // File filter mode: render the filter input in the status line
    if panel.filter.active {
        draw_file_filter_status(f, panel, area, ctx);
        return;
    }

    let error = panel.error.as_deref().unwrap_or("");
    // Count only files that are actually listed, so the totals do not include
    // entries withheld by the hidden-file setting.
    let file_count = panel.listed_file_count();
    let dir_count = panel
        .entries
        .iter()
        .enumerate()
        .filter(|(i, e)| e.is_dir && e.name != ".." && panel.is_visible(*i))
        .count();
    let selected_count = panel.entries.iter().filter(|e| e.selected).count();

    // Build the active-filter label (rendered in yellow, separated from the rest)
    let filter_label = panel.filter.applied.as_deref().map(|pattern| {
        format!(
            "Filter: {} {}/{} files, ",
            pattern,
            panel.visible_file_count(),
            file_count
        )
    });

    let status = if !error.is_empty() {
        error.to_string()
    } else if !panel.search.buffer.is_empty() {
        format!(
            "{} | {} matches",
            panel.search.buffer,
            panel.search.matching_indices.len()
        )
    } else {
        let items_info = if let Some((msg, instant)) = &panel.status_msg
            && instant.elapsed() < std::time::Duration::from_secs(3)
        {
            msg.clone()
        } else if panel.filter.is_active() {
            // File count is already shown in the filter label; only show dirs here.
            format!("{dir_count} dirs")
        } else {
            format!("{file_count} files, {dir_count} dirs")
        };

        if selected_count > 0 {
            format!("{items_info} | {selected_count} selected")
        } else {
            items_info
        }
    };

    // Use the same background as file/directory rows (surface1)
    let fg = if error.is_empty() {
        if !panel.search.buffer.is_empty() {
            Color::Rgb(
                ctx.palette.yellow.r,
                ctx.palette.yellow.g,
                ctx.palette.yellow.b,
            )
        } else if let Some((_, instant)) = &panel.status_msg
            && instant.elapsed() < std::time::Duration::from_secs(3)
        {
            Color::Rgb(
                ctx.palette.yellow.r,
                ctx.palette.yellow.g,
                ctx.palette.yellow.b,
            )
        } else {
            Color::Rgb(ctx.palette.text.r, ctx.palette.text.g, ctx.palette.text.b)
        }
    } else {
        Color::Rgb(ctx.palette.red.r, ctx.palette.red.g, ctx.palette.red.b)
    };
    // Move status line one character to the right and reduce width by 2 (1 for left offset, 1 for right margin)
    let status_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };

    let is_root = is_root_user(panel);
    let panel_bg = panel_bg_color(ctx.palette, ctx.active, is_root, ctx.borders);

    // Clear the status area first to prevent artifacts
    f.render_widget(Block::default().style(Style::default().bg(panel_bg)), area);

    match ctx.side {
        crate::app_state::tabs::PanelSide::Left => {
            draw_left_panel_status(
                f,
                ctx,
                status_area,
                status.as_ref(),
                fg,
                filter_label.as_deref(),
            );
        }
        crate::app_state::tabs::PanelSide::Right => {
            draw_right_panel_status(
                f,
                ctx,
                status_area,
                status.as_ref(),
                fg,
                filter_label.as_deref(),
            );
        }
    }
}

fn draw_file_filter_status(
    f: &mut ratatui::Frame,
    panel: &Tab,
    area: Rect,
    ctx: &PanelStatusContext,
) {
    let is_root = is_root_user(panel);
    let panel_bg = panel_bg_color(ctx.palette, ctx.active, is_root, ctx.borders);

    // Clear the status area
    f.render_widget(Block::default().style(Style::default().bg(panel_bg)), area);

    let status_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };

    let yellow = Color::Rgb(
        ctx.palette.yellow.r,
        ctx.palette.yellow.g,
        ctx.palette.yellow.b,
    );

    let pattern = &panel.filter.pattern;
    let cursor_pos = panel.filter.cursor_position;

    // Check if pattern is a valid glob (non-empty pattern only)
    let error_msg = if pattern.trim().is_empty() {
        None
    } else {
        crate::state::compile_glob(&crate::state::effective_glob(pattern.trim()))
            .err()
            .map(|e| format!("{e}"))
    };

    let red = Color::Rgb(ctx.palette.red.r, ctx.palette.red.g, ctx.palette.red.b);

    // Build the input line: /pattern with a block cursor at cursor_position
    let prefix = Span::styled("/".to_string(), Style::default().fg(yellow));

    let before_cursor: String = pattern.chars().take(cursor_pos).collect();
    let cursor_char = pattern.chars().nth(cursor_pos);
    let after_cursor: String = pattern.chars().skip(cursor_pos + 1).collect();

    let fg = if error_msg.is_some() { red } else { yellow };

    let before_span = Span::styled(before_cursor, Style::default().fg(fg));
    let cursor_span = Span::styled(
        cursor_char.map_or("▎".to_string(), |c| c.to_string()),
        Style::default()
            .fg(fg)
            .add_modifier(ratatui::style::Modifier::REVERSED),
    );
    let after_span = Span::styled(after_cursor, Style::default().fg(fg));

    let mut spans = vec![prefix, before_span, cursor_span, after_span];

    if let Some(ref err) = error_msg {
        spans.push(Span::styled(format!("  {err}"), Style::default().fg(red)));
    }

    let line = ratatui::text::Line::from(spans);
    let paragraph = ratatui::widgets::Paragraph::new(line);
    f.render_widget(paragraph, status_area);
}

fn build_status_line(
    filter: Option<&str>,
    status: &str,
    fg: Color,
    yellow: Color,
) -> ratatui::text::Line<'static> {
    let filter_span = filter.map(|f| Span::styled(f.to_string(), Style::default().fg(yellow)));
    let status_span = Span::styled(status.to_string(), Style::default().fg(fg));

    let mut spans = Vec::new();
    if let Some(fs) = filter_span {
        spans.push(fs);
    }
    spans.push(status_span);
    ratatui::text::Line::from(spans)
}

fn draw_left_panel_status(
    f: &mut ratatui::Frame,
    ctx: &PanelStatusContext,
    status_area: Rect,
    status: &str,
    fg: Color,
    filter: Option<&str>,
) {
    let tasks = ctx.task_manager.get_tasks();
    let running_count = tasks
        .iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::Running))
        .count();

    let yellow = Color::Rgb(
        ctx.palette.yellow.r,
        ctx.palette.yellow.g,
        ctx.palette.yellow.b,
    );

    if running_count > 0 {
        let text = if running_count == 1 {
            "1 task running".to_string()
        } else {
            format!("{running_count} tasks running")
        };
        let text_width = u16::try_from(text.len()).unwrap_or(u16::MAX);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(text_width)])
            .split(status_area);

        // File Info (Left)
        let paragraph =
            ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow));
        f.render_widget(paragraph, chunks[0]);

        // Task Info (Right)
        let p = ratatui::widgets::Paragraph::new(text)
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::Rgb(
                ctx.palette.yellow.r,
                ctx.palette.yellow.g,
                ctx.palette.yellow.b,
            )));
        f.render_widget(p, chunks[1]);
    } else {
        // Check for recently completed/failed/cancelled tasks
        let last_finished = tasks
            .iter()
            .filter(|t| t.completed_at.is_some())
            .max_by_key(|t| t.completed_at);

        if let Some(t) = last_finished {
            let (text, task_fg) = match &t.status {
                crate::tasks::TaskStatus::Completed => {
                    (String::new(), ctx.palette.green) // no text for task completed status
                }
                crate::tasks::TaskStatus::Failed(e) if !t.error_reported => {
                    (format!("Task failed: {e}"), ctx.palette.red)
                }
                crate::tasks::TaskStatus::Failed(_) => {
                    // Already on a panel status line; don't say it twice.
                    (String::new(), ctx.palette.red)
                }
                crate::tasks::TaskStatus::Cancelled => {
                    ("Task cancelled".to_string(), ctx.palette.yellow)
                }
                crate::tasks::TaskStatus::Running => unreachable!(),
            };
            let text_width = u16::try_from(text.len()).unwrap_or(u16::MAX);

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(text_width)])
                .split(status_area);

            // File Info (Left)
            let paragraph =
                ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow));
            f.render_widget(paragraph, chunks[0]);

            // Task Info (Right)
            let p = ratatui::widgets::Paragraph::new(text)
                .alignment(Alignment::Right)
                .style(Style::default().fg(Color::Rgb(task_fg.r, task_fg.g, task_fg.b)));
            f.render_widget(p, chunks[1]);
        } else {
            // No tasks, just file info
            let paragraph =
                ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow));
            f.render_widget(paragraph, status_area);
        }
    }
}

fn draw_right_panel_status(
    f: &mut ratatui::Frame,
    ctx: &PanelStatusContext,
    status_area: Rect,
    status: &str,
    fg: Color,
    filter: Option<&str>,
) {
    // Right Panel: Progress on Left, Files/Dirs on Right
    // (Task results are only shown on the left panel status bar)

    let yellow = Color::Rgb(
        ctx.palette.yellow.r,
        ctx.palette.yellow.g,
        ctx.palette.yellow.b,
    );

    // Check for active task progress
    let tasks = ctx.task_manager.get_tasks();
    let active_task = tasks
        .iter()
        .rev()
        .find(|t| matches!(t.status, crate::tasks::TaskStatus::Running));

    if let Some(t) = active_task {
        // Calculate available width for progress info
        // We need to leave room for the status message on the right
        let status_width = u16::try_from(status.chars().count()).unwrap_or(u16::MAX);
        let spacing = 2; // Extra space between progress and status
        let available_progress_width = status_area.width.saturating_sub(status_width + spacing);

        let progress_spans = get_task_progress_spans(
            t.progress,
            t.byte_progress,
            t.rsync,
            t.current_file.as_deref(),
            available_progress_width as usize,
            ctx.palette,
        );

        let text_width = u16::try_from(
            progress_spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>(),
        )
        .unwrap_or(u16::MAX);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(text_width), Constraint::Min(0)])
            .split(status_area);

        // Task Info (Left)
        let progress_line = ratatui::text::Line::from(progress_spans);
        let progress_paragraph =
            ratatui::widgets::Paragraph::new(progress_line).alignment(Alignment::Left);
        f.render_widget(progress_paragraph, chunks[0]);

        // File Info (Right)
        let paragraph =
            ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow))
                .alignment(Alignment::Right);
        f.render_widget(paragraph, chunks[1]);
    } else {
        // Default: just file info (Right aligned)
        let paragraph =
            ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow))
                .alignment(Alignment::Right);
        f.render_widget(paragraph, status_area);
    }
}

fn get_task_progress_spans(
    progress: Option<(usize, usize)>,
    byte_progress: Option<(u64, u64)>,
    rsync: bool,
    current_file: Option<&str>,
    max_width: usize,
    palette: &ThemePalette,
) -> Vec<Span<'static>> {
    let mut progress_spans = Vec::new();
    let yellow = Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b);

    let mut item_progress_str = String::new();
    if let Some((p, t)) = progress
        && t > 0
    {
        let percent = (p * 100).checked_div(t).unwrap_or(0);
        let left = t.saturating_sub(p);
        item_progress_str = format!("{percent}% ({left} left) ");
    }

    let mut byte_progress_str = String::new();
    if let Some((p_bytes, t_bytes)) = byte_progress
        && t_bytes > 0
    {
        let percent = (p_bytes * 100)
            .checked_div(t_bytes)
            .map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX));
        byte_progress_str = format!(
            "[{}% of {}] ",
            percent,
            crate::fs::utils::format_size(Some(t_bytes), false, false).trim()
        );
    }

    let mut used_width = item_progress_str.chars().count() + byte_progress_str.chars().count();
    let rsync_str = if rsync { " [rsync] " } else { "" };
    used_width += rsync_str.chars().count();

    let mut separator = "";
    if !item_progress_str.is_empty() && current_file.is_some() {
        separator = "| ";
        used_width += separator.chars().count();
    }

    if !item_progress_str.is_empty() {
        progress_spans.push(Span::styled(item_progress_str, Style::default().fg(yellow)));
    }

    if !rsync_str.is_empty() {
        progress_spans.push(Span::styled(
            rsync_str,
            Style::default().fg(Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)),
        ));
    }

    if let Some(file) = current_file {
        let text_fg = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
        if !separator.is_empty() {
            progress_spans.push(Span::styled(separator, Style::default().fg(text_fg)));
        }

        let available_for_file = max_width.saturating_sub(used_width);
        // Filename should take at least some space if possible, or be empty if no room at all
        if available_for_file > 0 {
            let truncated_file = format!(
                "{} ",
                truncate_middle_with_ellipsis(file, available_for_file.saturating_sub(1))
            );
            progress_spans.push(Span::styled(truncated_file, Style::default().fg(text_fg)));
        }
    }

    if !byte_progress_str.is_empty() {
        progress_spans.push(Span::styled(byte_progress_str, Style::default().fg(yellow)));
    }

    progress_spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Rgb;

    #[test]
    fn test_task_progress_truncation() {
        let palette = crate::theme::catppuccin_macchiato();

        // Case 1: No truncation needed
        // Progress strings: "50% (5 left) " (13) + "| " (2) + "[50% of 1000 B] " (~16) = ~31 overhead
        // "file.txt " = 9 chars. Total needed ~40.
        let max_width = 60;
        let spans = get_task_progress_spans(
            Some((5, 10)),
            Some((500, 1000)),
            false,
            Some("file.txt"),
            max_width,
            &palette,
        );
        let total_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(total_width <= max_width);
        assert!(spans.iter().any(|s| s.content.contains("file.txt")));

        // Case 2: Truncation needed for long filename
        // Overhead ~31. Max width 45. Available ~14.
        // File "very...txt" > 14. Should truncate.
        let case2_width = 45;
        let long_file = "very_long_filename_that_definitely_needs_truncation.txt";
        let spans = get_task_progress_spans(
            Some((5, 10)),
            Some((500, 1000)),
            false,
            Some(long_file),
            case2_width,
            &palette,
        );
        let total_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(total_width <= case2_width);
        // The filename should be truncated
        assert!(spans.iter().any(|s| s.content.contains("…")));

        // Case 3: Very narrow width
        // Max width 10. Overhead ~31. File should be dropped.
        // Total width will exceed max_width because progress bars are not truncated,
        // but we verify file is dropped.
        let narrow_width = 10;
        let spans = get_task_progress_spans(
            Some((5, 10)),
            Some((500, 1000)),
            false,
            Some("short.txt"),
            narrow_width,
            &palette,
        );
        // We expect the file to be absent
        assert!(!spans.iter().any(|s| s.content.contains("short.txt")));
    }

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
        super::name_col_width_for(total, attributes)
    }

    fn fg_of(style: Style) -> Option<Color> {
        style.fg
    }

    fn entry(name: &str, is_dir: bool, is_symlink: bool, attributes: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            is_dir,
            is_symlink,
            size: None,
            modified: None,
            attributes: attributes.to_string(),
            selected: false,
        }
    }

    /// [`style_for_entry`] for a filesystem that follows the Unix conventions:
    /// local on Unix, and every remote and archive tab on any host.
    fn unix_entry_style(entry: &FileEntry, current_dir: &Path, palette: &ThemePalette) -> Style {
        style_for_entry(entry, current_dir, palette, true)
    }

    /// [`style_for_icon`] for the same Unix-conventions filesystem.
    fn unix_icon_style(entry: &FileEntry, palette: &ThemePalette, name_style: Style) -> Style {
        style_for_icon(entry, palette, name_style, true)
    }

    /// [`style_for_entry`] for a local Windows filesystem, which reads the DOS
    /// attribute flags and has no dotfile convention.
    fn local_entry_style(entry: &FileEntry, current_dir: &Path, palette: &ThemePalette) -> Style {
        style_for_entry(entry, current_dir, palette, false)
    }

    /// [`style_for_icon`] for a local Windows filesystem.
    fn local_icon_style(entry: &FileEntry, palette: &ThemePalette, name_style: Style) -> Style {
        style_for_icon(entry, palette, name_style, false)
    }

    #[test]
    fn test_symlink_uses_its_own_color() {
        let palette = crate::theme::catppuccin_macchiato();
        let current_dir = Path::new("/nonexistent");
        let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

        // A real directory stays blue and a plain executable stays green.
        let dir = entry("real", true, false, "drwxr-xr-x ");
        assert_eq!(
            fg_of(unix_entry_style(&dir, current_dir, &palette)),
            Some(rgb(palette.blue))
        );
        let exec = entry("run", false, false, "-rwxr-xr-x");
        assert_eq!(
            fg_of(unix_entry_style(&exec, current_dir, &palette)),
            Some(rgb(palette.green))
        );

        // A link to a directory is not blue: the cyan marks it as a link even
        // though it is enterable and sorts with directories.
        let linked_dir = entry("link", true, true, "lrwxrwxrwx ");
        let dir_style = fg_of(unix_entry_style(&linked_dir, current_dir, &palette));
        assert_eq!(dir_style, Some(rgb(palette.cyan)));
        assert_ne!(dir_style, Some(rgb(palette.blue)));
        assert_ne!(dir_style, Some(rgb(palette.green)));

        // A link to a file uses the same color, and never the executable green.
        let linked_file = entry("link.txt", false, true, "lrwxrwxrwx ");
        let file_style = fg_of(unix_entry_style(&linked_file, current_dir, &palette));
        assert_eq!(file_style, Some(rgb(palette.cyan)));
        assert_ne!(file_style, Some(rgb(palette.green)));
        assert_ne!(file_style, Some(rgb(palette.text)));

        // A broken link falls back to the same cyan.
        let broken = entry("broken", false, true, "lrwxrwxrwx ");
        assert_eq!(
            fg_of(unix_entry_style(&broken, current_dir, &palette)),
            Some(rgb(palette.cyan))
        );
    }

    #[test]
    fn test_every_theme_defines_a_symlink_color() {
        for name in crate::theme::THEME_NAMES {
            let palette = crate::theme::get_theme(name).unwrap();
            assert_ne!(palette.cyan.r, 0, "{name} has an unset symlink color");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn test_hidden_entries_use_a_de_emphasized_color() {
        let current_dir = Path::new("/nonexistent");
        let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

        for name in crate::theme::THEME_NAMES {
            let palette = crate::theme::get_theme(name).unwrap();

            // A hidden file is muted to the subtext color instead of the normal
            // text color.
            let hidden_file = entry(".env", false, false, "-rw-r--r--");
            assert_eq!(
                fg_of(unix_entry_style(&hidden_file, current_dir, &palette)),
                Some(rgb(palette.subtext)),
                "{name} does not mute hidden files"
            );

            // Hidden directories use the same muted color as hidden files.
            let hidden_dir = entry(".git", true, false, "drwxr-xr-x ");
            assert_eq!(
                fg_of(unix_entry_style(&hidden_dir, current_dir, &palette)),
                Some(rgb(palette.subtext)),
                "{name} does not mute hidden directories"
            );

            // Hiding a file wins over the executable green.
            let hidden_exec = entry(".hidden.sh", false, false, "-rwxr-xr-x");
            assert_eq!(
                fg_of(unix_entry_style(&hidden_exec, current_dir, &palette)),
                Some(rgb(palette.subtext)),
                "{name} colors a hidden executable green"
            );

            // A visible file keeps the normal text color.
            let plain = entry("env", false, false, "-rw-r--r--");
            assert_eq!(
                fg_of(unix_entry_style(&plain, current_dir, &palette)),
                Some(rgb(palette.text)),
                "{name} does not keep the text color for visible files"
            );

            // A visible directory keeps the directory blue.
            let dir = entry("git", true, false, "drwxr-xr-x ");
            assert_eq!(
                fg_of(unix_entry_style(&dir, current_dir, &palette)),
                Some(rgb(palette.blue)),
                "{name} does not keep the directory color for visible directories"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_dot_names_keep_their_regular_color_on_windows() {
        let current_dir = Path::new("/nonexistent");
        let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

        for name in crate::theme::THEME_NAMES {
            let palette = crate::theme::get_theme(name).unwrap();

            // A local Windows filesystem has no dotfile convention, so a leading
            // dot must not mute the entry the way it does on Unix. These entries
            // carry no DOS hidden flag, so nothing marks them hidden.
            let dot_file = entry(".env", false, false, "------");
            let file_style = local_entry_style(&dot_file, current_dir, &palette);
            assert_eq!(
                fg_of(file_style),
                Some(rgb(palette.text)),
                "{name} mutes a dot-prefixed file on Windows"
            );
            assert_eq!(
                fg_of(local_icon_style(&dot_file, &palette, file_style)),
                Some(rgb(palette.text)),
                "{name} changes the icon of a dot-prefixed file on Windows"
            );

            let dot_dir = entry(".git", true, false, "d-----");
            let dir_style = local_entry_style(&dot_dir, current_dir, &palette);
            assert_eq!(
                fg_of(dir_style),
                Some(rgb(palette.blue)),
                "{name} mutes a dot-prefixed directory on Windows"
            );
            assert_eq!(
                fg_of(local_icon_style(&dot_dir, &palette, dir_style)),
                Some(rgb(palette.blue)),
                "{name} changes the icon of a dot-prefixed directory on Windows"
            );

            // An executable keeps its green even behind a leading dot.
            let dot_exec = entry(".build.sh", false, false, "-rwxr-xr-x");
            assert_eq!(
                fg_of(local_entry_style(&dot_exec, current_dir, &palette)),
                Some(rgb(palette.green)),
                "{name} mutes a dot-prefixed executable on Windows"
            );

            // A dot-prefixed symlink keeps its cyan link marker.
            let dot_link = entry(".link", true, true, "lrwxrwxrwx ");
            let link_style = local_entry_style(&dot_link, current_dir, &palette);
            assert_eq!(
                fg_of(link_style),
                Some(rgb(palette.cyan)),
                "{name} mutes a dot-prefixed symlink on Windows"
            );
            assert_eq!(
                fg_of(local_icon_style(&dot_link, &palette, link_style)),
                Some(rgb(palette.cyan)),
                "{name} changes the icon of a dot-prefixed symlink on Windows"
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn test_hidden_directory_icon_keeps_the_directory_blue() {
        let current_dir = Path::new("/nonexistent");
        let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

        for name in crate::theme::THEME_NAMES {
            let palette = crate::theme::get_theme(name).unwrap();

            // A hidden directory keeps the standard directory blue on its icon
            // while its name stays muted.
            let hidden_dir = entry(".git", true, false, "drwxr-xr-x ");
            let name_style = unix_entry_style(&hidden_dir, current_dir, &palette);
            assert_eq!(
                fg_of(name_style),
                Some(rgb(palette.subtext)),
                "{name} does not mute the name of a hidden directory"
            );
            assert_eq!(
                fg_of(unix_icon_style(&hidden_dir, &palette, name_style)),
                Some(rgb(palette.blue)),
                "{name} does not keep the directory blue on the icon"
            );

            // Hidden files have no directory icon to keep, so theirs matches the
            // muted name.
            let hidden_file = entry(".env", false, false, "-rw-r--r--");
            let file_style = unix_entry_style(&hidden_file, current_dir, &palette);
            assert_eq!(
                fg_of(unix_icon_style(&hidden_file, &palette, file_style)),
                fg_of(file_style),
                "{name} does not keep hidden file icons muted"
            );

            // A visible directory is unchanged: icon and name both stay blue.
            let dir = entry("git", true, false, "drwxr-xr-x ");
            let dir_style = unix_entry_style(&dir, current_dir, &palette);
            assert_eq!(
                fg_of(unix_icon_style(&dir, &palette, dir_style)),
                Some(rgb(palette.blue)),
                "{name} changed the icon of a visible directory"
            );

            // A hidden symlink to a directory keeps its cyan link marker rather
            // than borrowing the plain directory blue.
            let linked_dir = entry(".link", true, true, "lrwxrwxrwx ");
            let link_style = unix_entry_style(&linked_dir, current_dir, &palette);
            assert_eq!(
                fg_of(unix_icon_style(&linked_dir, &palette, link_style)),
                Some(rgb(palette.cyan)),
                "{name} dropped the link color for a hidden symlinked directory"
            );
        }
    }

    #[test]
    fn test_windows_hidden_attribute_mutes_the_entry() {
        let current_dir = Path::new("/nonexistent");
        let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

        for name in crate::theme::THEME_NAMES {
            let palette = crate::theme::get_theme(name).unwrap();

            // A local Windows filesystem marks a hidden file with FILE_ATTRIBUTE_HIDDEN
            // rather than a leading dot, so the name is muted without any dot.
            let hidden_file = entry("AppData", false, false, "--h---");
            let file_style = local_entry_style(&hidden_file, current_dir, &palette);
            assert_eq!(
                fg_of(file_style),
                Some(rgb(palette.subtext)),
                "{name} does not mute a file hidden via the DOS attribute"
            );
            assert_eq!(
                fg_of(local_icon_style(&hidden_file, &palette, file_style)),
                fg_of(file_style),
                "{name} does not keep a DOS-hidden file icon muted"
            );

            // A hidden Windows directory is muted too, icon aside.
            let hidden_dir = entry("AppData", true, false, "d-h---");
            let dir_style = local_entry_style(&hidden_dir, current_dir, &palette);
            assert_eq!(
                fg_of(dir_style),
                Some(rgb(palette.subtext)),
                "{name} does not mute a directory hidden via the DOS attribute"
            );
            assert_eq!(
                fg_of(local_icon_style(&hidden_dir, &palette, dir_style)),
                Some(rgb(palette.blue)),
                "{name} does not keep the directory blue on a DOS-hidden icon"
            );

            // An ordinary Windows directory is untouched.
            let dir = entry("Documents", true, false, "d----");
            let visible_style = local_entry_style(&dir, current_dir, &palette);
            assert_eq!(
                fg_of(visible_style),
                Some(rgb(palette.blue)),
                "{name} muted a plain Windows directory"
            );
        }
    }

    #[test]
    fn test_navigation_entries_are_not_treated_as_hidden() {
        let current_dir = Path::new("/nonexistent");
        let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
        let palette = crate::theme::catppuccin_macchiato();

        for name in [".", ".."] {
            let nav = entry(name, true, false, "drwxr-xr-x ");
            assert!(
                !crate::fs::utils::is_hidden(name, &nav.attributes, true),
                "{name} should not count as hidden"
            );
            assert_eq!(
                fg_of(unix_entry_style(&nav, current_dir, &palette)),
                Some(rgb(palette.blue)),
                "{name} should keep the directory color"
            );
        }
    }

    #[test]
    fn test_remote_dotfiles_are_muted_on_every_platform() {
        // A remote tab lists Unix permissions and never sets a DOS hidden flag,
        // so its dotfiles must be muted even when the client runs on Windows.
        // A `cfg!(windows)` gate left them fully colored, which is the visible
        // half of the toggle doing nothing in a remote tab.
        let current_dir = Path::new("/nonexistent");
        let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
        let palette = crate::theme::catppuccin_macchiato();

        for (name, is_dir, attributes, local_fg) in [
            (".env", false, "-rw-r--r--", rgb(palette.text)),
            (".git", true, "drwxr-xr-x ", rgb(palette.blue)),
            (
                ".hidden.sh",
                false,
                "-rwxr-xr-x",
                rgb(palette.green), // an `x` bit reads as executable everywhere
            ),
        ] {
            let remote = entry(name, is_dir, false, attributes);
            let style = unix_entry_style(&remote, current_dir, &palette);
            assert_eq!(
                fg_of(style),
                Some(rgb(palette.subtext)),
                "{name:?} is not muted in a remote tab"
            );
            // A hidden remote directory keeps the directory blue on its icon, so
            // the row still reads as a directory.
            let icon_fg = fg_of(unix_icon_style(&remote, &palette, style));
            assert_eq!(
                icon_fg,
                if is_dir {
                    Some(rgb(palette.blue))
                } else {
                    Some(rgb(palette.subtext))
                },
                "{name:?} has the wrong icon color in a remote tab"
            );
            // A local Windows filesystem of the same shape keeps them unmuted.
            assert_eq!(
                fg_of(local_entry_style(&remote, current_dir, &palette)),
                Some(local_fg),
                "{name:?} must not be muted locally on Windows without the DOS flag"
            );
        }
    }

    #[test]
    fn test_attributes_type_char_colors() {
        let palette = crate::theme::catppuccin_macchiato();
        let text_fg = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
        let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

        // `l` marks a symlink with the same cyan as its name, for both a linked
        // directory and a linked file.
        for attrs in ["lrwxrwxrwx", "lrwxrwxrwx "] {
            assert_eq!(
                attributes_char_style('l', &palette, text_fg).fg,
                Some(rgb(palette.cyan)),
                "unexpected color for the link marker of {attrs:?}"
            );
        }
        // `d` keeps the directory blue.
        assert_eq!(
            attributes_char_style('d', &palette, text_fg).fg,
            Some(rgb(palette.blue))
        );
        // `x` keeps the executable green.
        assert_eq!(
            attributes_char_style('x', &palette, text_fg).fg,
            Some(rgb(palette.green))
        );
        // `r`/`-` stay muted, `w` uses the normal text color.
        assert_eq!(
            attributes_char_style('r', &palette, text_fg).fg,
            Some(rgb(palette.overlay0))
        );
        assert_eq!(
            attributes_char_style('-', &palette, text_fg).fg,
            Some(rgb(palette.overlay0))
        );
        assert_eq!(
            attributes_char_style('w', &palette, text_fg).fg,
            Some(text_fg)
        );
    }
}
