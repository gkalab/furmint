use crate::app::{Tab, TabManager};
use crate::fs_ops::{format_modified, format_size};
use crate::theme::ThemePalette;
use crate::ui_utils::{truncate_middle_with_ellipsis, truncate_path_with_ellipsis};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

/// Draw the tab bar for a panel
pub fn draw_tab_bar(
    f: &mut ratatui::Frame,
    tab_manager: &TabManager,
    area: Rect,
    palette: &ThemePalette,
    active: bool,
) {
    if area.height == 0 {
        return;
    }

    let mut spans = Vec::new();
    let tab_count = tab_manager.tabs.len();

    for (idx, tab) in tab_manager.tabs.iter().enumerate() {
        // Get the last component of the path
        let tab_title = tab
            .current_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("/");

        // Truncate if too long (max 15 chars)
        let truncated_title = if tab_title.len() > 15 {
            format!("{}…", &tab_title[..12])
        } else {
            tab_title.to_string()
        };

        // Style based on whether this tab is active
        let is_active_tab = idx == tab_manager.active_tab_index;
        let (fg, bg) = if is_active_tab && active {
            // Active tab in active panel
            (
                Color::Rgb(palette.base.r, palette.base.g, palette.base.b),
                Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b),
            )
        } else if is_active_tab {
            // Active tab in inactive panel
            (
                Color::Rgb(palette.text.r, palette.text.g, palette.text.b),
                Color::Rgb(palette.surface1.r, palette.surface1.g, palette.surface1.b),
            )
        } else {
            // Inactive tab
            (
                Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b),
                Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b),
            )
        };

        // Add tab with padding
        spans.push(Span::styled(
            format!(" {} ", truncated_title),
            Style::default().fg(fg).bg(bg),
        ));

        // Add separator between tabs
        if idx < tab_count - 1 {
            spans.push(Span::raw(" "));
        }
    }

    let line = Line::from(spans);
    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let paragraph = ratatui::widgets::Paragraph::new(line).style(Style::default().bg(bg_color));
    f.render_widget(paragraph, area);
}

pub fn draw_panel(
    f: &mut ratatui::Frame,
    panel: &Tab,
    active: bool,
    area: Rect,
    palette: &ThemePalette,
) {
    // Compute max width for Size column
    let size_width = panel
        .entries
        .iter()
        .map(|e| format_size(e.size, e.is_dir, e.is_symlink).len())
        .max()
        .unwrap_or(4);
    let size_header = format!("{:>width$}", "Size", width = size_width);
    let header = ["Name", &size_header, "Modified", "Attributes"];

    let text_fg = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    // Calculate available width for name column
    let total_table_width = area.width as usize;
    let name_col_width = if total_table_width > (10 + 19 + 10) {
        total_table_width - (10 + 19 + 10)
    } else {
        10 // minimum width for name
    };

    let rows = panel.entries.iter().map(|e| {
        // Account for ratatui border: subtract 2 from available width
        let visible_name_width = if name_col_width > 2 {
            name_col_width - 2
        } else {
            1
        };
        let truncated_name = truncate_middle_with_ellipsis(&e.name, visible_name_width);
        let mut name_cell = Cell::from(truncated_name);
        let full_path = panel.current_dir.join(&e.name);
        let name_style = if e.is_dir {
            Style::default().fg(Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b))
        } else if is_executable(&full_path, e) {
            Style::default().fg(Color::Rgb(
                palette.green.r,
                palette.green.g,
                palette.green.b,
            ))
        } else {
            Style::default().fg(text_fg)
        };
        name_cell = name_cell.style(name_style);
        Row::new(vec![
            name_cell,
            Cell::from(format_size(e.size, e.is_dir, e.is_symlink))
                .style(Style::default().fg(text_fg)),
            Cell::from(format_modified(e.modified)).style(Style::default().fg(text_fg)),
            Cell::from(e.attributes.clone()).style(Style::default().fg(text_fg)),
        ])
    });

    // Helper to detect executables
    fn is_executable(full_path: &std::path::Path, e: &crate::fs_ops::FileEntry) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::symlink_metadata(full_path) {
                let mode = meta.permissions().mode();
                mode & 0o111 != 0 && !e.is_dir
            } else {
                false
            }
        }
        #[cfg(windows)]
        {
            let lower = e.name.to_lowercase();
            (lower.ends_with(".exe") || lower.ends_with(".bat") || lower.ends_with(".cmd"))
                && !e.is_dir
        }
    }

    let border_color = if active {
        Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)
    } else {
        Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
    };

    let title_width = area.width.saturating_sub(2) as usize;
    let panel_title = truncate_path_with_ellipsis(&panel.current_dir, title_width);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title)
        .border_style(Style::default().fg(border_color));
    let widths = [
        Constraint::Min(10),    // Name: dynamic, at least 10
        Constraint::Length(7),  // Size: always 7 (right-aligned)
        Constraint::Length(19), // Modified: always 19
        Constraint::Length(10), // Attributes: always 10
    ];
    let highlight_bg = if active {
        Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b)
    } else {
        // Use a lighter color for the selection line of the inactive panel
        Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b)
    };
    let highlight_fg = if !palette.is_dark && active {
        // For light themes, use the base background color for text on the dark selection background
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    } else {
        Color::Rgb(palette.text.r, palette.text.g, palette.text.b)
    };
    let table = Table::new(rows, widths)
        .header(Row::new(header).style(Style::default().fg(Color::Rgb(
            palette.yellow.r,
            palette.yellow.g,
            palette.yellow.b,
        ))))
        .block(block)
        .row_highlight_style(Style::default().bg(highlight_bg).fg(highlight_fg));
    f.render_stateful_widget(
        table,
        area,
        &mut TableState::default().with_selected(Some(panel.cursor)),
    );

    // Draw selection markers over the left border
    let yellow_color = Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b);
    let border_color = if active {
        Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)
    } else {
        Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
    };

    // Calculate which entries are visible in the current view
    let visible_rows = area.height.saturating_sub(2) as usize; // -2 for top/bottom borders
    let start_idx = if panel.cursor >= visible_rows {
        panel.cursor - visible_rows + 1
    } else {
        0
    };

    for (idx, entry) in panel
        .entries
        .iter()
        .enumerate()
        .skip(start_idx)
        .take(visible_rows)
    {
        let row_y = area.y + 2 + (idx - start_idx) as u16; // +2 for border and header

        if row_y >= area.y + area.height - 1 {
            break; // Don't draw past the bottom border
        }

        let marker = if entry.selected {
            Span::styled("█", Style::default().fg(yellow_color))
        } else {
            // Restore the border character
            Span::styled("│", Style::default().fg(border_color))
        };

        f.render_widget(
            Line::from(marker),
            Rect {
                x: area.x,
                y: row_y,
                width: 1,
                height: 1,
            },
        );
    }

    // Draw unobtrusive vertical scrollbar if needed
    let visible_rows = area.height.saturating_sub(1) as usize; // 1 for header
    let total_entries = panel.entries.len();
    if total_entries > visible_rows {
        use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
        let mut scrollbar_state = ScrollbarState::new(total_entries)
            .viewport_content_length(visible_rows)
            .position(panel.cursor);
        let scrollbar_color =
            Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);
        let scroll_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1, // +1 for header
            width: 1,
            height: visible_rows as u16,
        };
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .style(Style::default().fg(scrollbar_color));
        f.render_stateful_widget(scrollbar, scroll_area, &mut scrollbar_state);
    }
}

pub fn draw_panel_status(
    f: &mut ratatui::Frame,
    panel: &Tab,
    area: Rect,
    palette: &ThemePalette,
    _active: bool,
) {
    let error = panel.error.as_ref().map(|s| s.as_str()).unwrap_or("");
    let file_count = panel.entries.iter().filter(|e| !e.is_dir).count();
    let dir_count = panel
        .entries
        .iter()
        .filter(|e| e.is_dir && e.name != "..")
        .count();
    let selected_count = panel.entries.iter().filter(|e| e.selected).count();

    let status = if !error.is_empty() {
        format!("{}", error)
    } else if selected_count > 0 {
        format!(
            "{} files, {} dirs | {} selected",
            file_count, dir_count, selected_count
        )
    } else {
        format!("{} files, {} dirs", file_count, dir_count)
    };
    // Use the same background as file/directory rows (surface1)
    let fg = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    // Move status line one character to the right and remove background color
    let status_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(1),
        height: area.height,
    };
    let paragraph = ratatui::widgets::Paragraph::new(status).style(Style::default().fg(fg)); // No background color
    f.render_widget(paragraph, status_area);
}

pub fn draw_file_viewer(
    f: &mut ratatui::Frame,
    viewer: &crate::app::FileViewerState,
    area: Rect,
    palette: &ThemePalette,
) {
    use syntect::easy::HighlightLines;

    let block = Block::default()
        .borders(Borders::ALL)
        .title(viewer.path.to_string_lossy())
        .border_style(Style::default().fg(if viewer.focused {
            Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)
        } else {
            Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
        }));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    if viewer.content.is_empty() {
        return;
    }

    // Use cached syntax name
    let syntax = viewer
        .syntax_name
        .as_ref()
        .and_then(|name| viewer.syntax_set.find_syntax_by_name(name))
        .unwrap_or_else(|| viewer.syntax_set.find_syntax_plain_text());

    let mut h = HighlightLines::new(syntax, &viewer.theme);

    let visible_lines = inner_area.height as usize;
    let max_lines = viewer.content.len();
    let start_line = viewer.scroll_offset;
    let end_line = (start_line + visible_lines).min(max_lines);

    for (i, line) in viewer.content[start_line..end_line].iter().enumerate() {
        let ranges: Vec<(syntect::highlighting::Style, &str)> = h
            .highlight_line(line, &viewer.syntax_set)
            .unwrap_or_default();
        let spans: Vec<Span> = ranges
            .into_iter()
            .map(|(style, text)| {
                let fg = style.foreground;
                Span::styled(text, Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b)))
            })
            .collect();

        f.render_widget(
            Line::from(spans),
            Rect {
                x: inner_area.x,
                y: inner_area.y + i as u16,
                width: inner_area.width,
                height: 1,
            },
        );
    }

    // Scrollbar
    if max_lines > visible_lines {
        use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
        let mut scrollbar_state = ScrollbarState::new(max_lines)
            .viewport_content_length(visible_lines)
            .position(viewer.scroll_offset);
        let scrollbar_color =
            Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);
        let scroll_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .style(Style::default().fg(scrollbar_color));
        f.render_stateful_widget(scrollbar, scroll_area, &mut scrollbar_state);
    }
}
