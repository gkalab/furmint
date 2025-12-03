use ratatui::prelude::*;
use ratatui::widgets::{Table, Row, Cell, Block, Borders, TableState};
use crate::fs_ops::{format_size, format_modified};
use crate::app::{PanelState};
use catppuccin::Flavor;

pub fn draw_panel(
    f: &mut ratatui::Frame,
    panel: &PanelState,
    active: bool,
    area: Rect,
    palette: &catppuccin::Flavor,
) {
    // Compute max width for Size column
    let size_width = panel.entries.iter()
        .map(|e| format_size(e.size, e.is_dir, e.is_symlink).len())
        .max()
        .unwrap_or(4);
    let size_header = format!("{:>width$}", "Size", width = size_width);
    let header = ["Name", &size_header, "Modified", "Attributes"];
    let rows = panel.entries.iter().map(|e| {
        let mut name_cell = Cell::from(e.name.clone());
        let full_path = panel.current_dir.join(&e.name);
        let name_style = if e.is_dir {
            Style::default().fg(Color::Rgb(
                palette.colors.blue.rgb.r,
                palette.colors.blue.rgb.g,
                palette.colors.blue.rgb.b,
            ))
        } else if is_executable(&full_path, e) {
            Style::default().fg(Color::Rgb(
                palette.colors.green.rgb.r,
                palette.colors.green.rgb.g,
                palette.colors.green.rgb.b,
            ))
        } else {
            Style::default().fg(Color::Rgb(
                palette.colors.text.rgb.r,
                palette.colors.text.rgb.g,
                palette.colors.text.rgb.b,
            ))
        };
        name_cell = name_cell.style(name_style);
        Row::new(vec![
            name_cell,
            Cell::from(format_size(e.size, e.is_dir, e.is_symlink)),
            Cell::from(format_modified(e.modified)),
            Cell::from(e.attributes.clone()),
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
        (lower.ends_with(".exe") || lower.ends_with(".bat") || lower.ends_with(".cmd")) && !e.is_dir
    }
}



    let border_color = if active {
        Color::Rgb(
            palette.colors.blue.rgb.r,
            palette.colors.blue.rgb.g,
            palette.colors.blue.rgb.b,
        )
    } else {
        Color::Rgb(
            palette.colors.overlay0.rgb.r,
            palette.colors.overlay0.rgb.g,
            palette.colors.overlay0.rgb.b,
        )
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
        Color::Rgb(
            palette.colors.surface2.rgb.r,
            palette.colors.surface2.rgb.g,
            palette.colors.surface2.rgb.b,
        )
    } else {
        Color::Rgb(
            palette.colors.surface1.rgb.r,
            palette.colors.surface1.rgb.g,
            palette.colors.surface1.rgb.b,
        )
    };
    let highlight_fg = Color::Rgb(
        palette.colors.text.rgb.r,
        palette.colors.text.rgb.g,
        palette.colors.text.rgb.b,
    );
    let table = Table::new(rows, widths)
        .header(Row::new(header).style(Style::default().fg(Color::Rgb(
            palette.colors.yellow.rgb.r,
            palette.colors.yellow.rgb.g,
            palette.colors.yellow.rgb.b,
        ))))
        .block(block)
        .row_highlight_style(Style::default()
            .bg(highlight_bg)
            .fg(highlight_fg)
        );
    f.render_stateful_widget(table, area, &mut TableState::default().with_selected(Some(panel.selected)));

    // Draw unobtrusive vertical scrollbar if needed
    let visible_rows = area.height.saturating_sub(1) as usize; // 1 for header
    let total_entries = panel.entries.len();
    if total_entries > visible_rows {
        use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
        let mut scrollbar_state = ScrollbarState::new(total_entries)
            .viewport_content_length(visible_rows)
            .position(panel.selected);
        let scrollbar_color = Color::Rgb(
            palette.colors.overlay0.rgb.r,
            palette.colors.overlay0.rgb.g,
            palette.colors.overlay0.rgb.b,
        );
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
    palette: &Flavor,
    _active: bool,
) {
    let error = panel.error.as_ref().map(|s| s.as_str()).unwrap_or("");
    let file_count = panel.entries.iter().filter(|e| !e.is_dir).count();
    let dir_count = panel.entries.iter().filter(|e| e.is_dir && e.name != "..").count();
    let status = if !error.is_empty() {
        format!("{}", error)
    } else {
        format!("{} files, {} dirs", file_count, dir_count)
    };
    // Use the same background as file/directory rows (surface1)
    let bg = Color::Rgb(
        palette.colors.surface1.rgb.r,
        palette.colors.surface1.rgb.g,
        palette.colors.surface1.rgb.b,
    );
    let fg = Color::Rgb(
        palette.colors.text.rgb.r,
        palette.colors.text.rgb.g,
        palette.colors.text.rgb.b,
    );
    let paragraph = ratatui::widgets::Paragraph::new(status)
        .style(Style::default().fg(fg).bg(bg));
    f.render_widget(paragraph, area);
}
