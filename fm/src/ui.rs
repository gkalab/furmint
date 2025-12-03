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
        .map(|e| format_size(e.size, e.is_dir).len())
        .max()
        .unwrap_or(4);
    let size_header = format!("{:>width$}", "Size", width = size_width);
    let header = ["Name", &size_header, "Modified", "Attributes"];
    let rows = panel.entries.iter().map(|e| {
         Row::new(vec![
            Cell::from(e.name.clone()),
            Cell::from(format_size(e.size, e.is_dir)),
            Cell::from(format_modified(e.modified)),
            Cell::from(e.attributes.clone()),
        ])
    });
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
    let bg = Color::Rgb(
        palette.colors.surface2.rgb.r,
        palette.colors.surface2.rgb.g,
        palette.colors.surface2.rgb.b,
    );
    let fg = Color::Rgb(
        palette.colors.text.rgb.r,
        palette.colors.text.rgb.g,
        palette.colors.text.rgb.b,
    );
    let block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(bg).fg(fg));
    let paragraph = ratatui::widgets::Paragraph::new(status)
        .block(block)
        .style(Style::default().fg(fg));
    f.render_widget(paragraph, area);
}
