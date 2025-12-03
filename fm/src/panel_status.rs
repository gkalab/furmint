use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};
use crate::PanelState;
use catppuccin::Flavor;

pub fn draw_panel_status(
    f: &mut ratatui::Frame,
    panel: &PanelState,
    area: Rect,
    palette: &Flavor,
    active: bool,
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
        .borders(Borders::NONE);
    let paragraph = ratatui::widgets::Paragraph::new(status)
        .block(block)
        .style(Style::default().fg(fg));
    f.render_widget(paragraph, area);
}
