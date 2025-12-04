use crate::app::PanelState;
use crate::fs_ops::{format_modified, format_size};
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

pub fn draw_panel(
    f: &mut ratatui::Frame,
    panel: &PanelState,
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
    let rows = panel.entries.iter().map(|e| {
        let mut name_cell = Cell::from(e.name.clone());
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel.current_dir.to_string_lossy())
        .border_style(Style::default().fg(border_color));
    let widths = [
        Constraint::Percentage(40),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
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
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_symbol("▐")
            .track_symbol(Some(" "))
            .style(Style::default().fg(scrollbar_color))
            .thumb_style(Style::default().fg(scrollbar_color))
            .track_style(Style::default().fg(scrollbar_color))
            .begin_symbol(None)
            .end_symbol(None);
        f.render_stateful_widget(scrollbar, scroll_area, &mut scrollbar_state);
    }
}

pub fn draw_panel_status(
    f: &mut ratatui::Frame,
    panel: &PanelState,
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
    let status = if !error.is_empty() {
        format!("{}", error)
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
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_symbol("▐")
            .track_symbol(Some(" "))
            .style(Style::default().fg(scrollbar_color))
            .thumb_style(Style::default().fg(scrollbar_color))
            .track_style(Style::default().fg(scrollbar_color))
            .begin_symbol(None)
            .end_symbol(None);
        f.render_stateful_widget(scrollbar, scroll_area, &mut scrollbar_state);
    }
}
