use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph,
};
use std::path::PathBuf;

pub struct FuzzySearchState {
    pub is_visible: bool,
    pub input: String,
    pub filtered_dirs: Vec<PathBuf>,
    pub selected_index: usize,
    pub scroll_offset: usize,
}

impl FuzzySearchState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            input: String::new(),
            filtered_dirs: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
        }
    }

    pub fn reset(&mut self) {
        self.input.clear();
        self.filtered_dirs.clear();
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    pub fn move_selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_selection_down(&mut self) {
        if self.selected_index + 1 < self.filtered_dirs.len() {
            self.selected_index += 1;
        }
    }

    pub fn move_selection_page_up(&mut self, page_size: usize) {
        if self.selected_index >= page_size {
            self.selected_index -= page_size;
        } else {
            self.selected_index = 0;
        }
    }

    pub fn move_selection_page_down(&mut self, page_size: usize) {
        let max_idx = self.filtered_dirs.len().saturating_sub(1);
        if self.selected_index + page_size <= max_idx {
            self.selected_index += page_size;
        } else {
            self.selected_index = max_idx;
        }
    }

    pub fn get_selected_dir(&self) -> Option<PathBuf> {
        self.filtered_dirs.get(self.selected_index).cloned()
    }

    /// Update scroll offset to keep selected item visible
    pub fn update_scroll(&mut self, visible_rows: usize) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected_index - visible_rows + 1;
        }
    }
}

pub fn draw_fuzzy_search_popup(
    f: &mut ratatui::Frame,
    state: &mut FuzzySearchState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    // Calculate popup size (centered, 60% width, 50% height)
    let area = f.area();
    let popup_width = (area.width as f32 * 0.6).min(80.0) as u16;
    let popup_height = (area.height as f32 * 0.5).min(20.0) as u16;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    // Clear the popup area first - this is essential!
    f.render_widget(Clear, popup_area);

    // Now render the background
    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let clear_rect = ratatui::widgets::Block::default().style(Style::default().bg(bg_color));
    f.render_widget(clear_rect, popup_area);

    // Split popup into input area and list area (removed status line)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Input box
            Constraint::Min(1),    // Directory list
        ])
        .split(popup_area);

    // Draw input box (top section with top, left, right borders)
    let input_block = Block::default()
        .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
        .title("Select Directory")
        .border_style(Style::default().fg(Color::Rgb(
            palette.blue.r,
            palette.blue.g,
            palette.blue.b,
        )))
        .style(Style::default().bg(bg_color));
    let input_text = if state.input.is_empty() {
        Span::styled(
            "Type to search...",
            Style::default().fg(Color::Rgb(
                palette.overlay0.r,
                palette.overlay0.g,
                palette.overlay0.b,
            )),
        )
    } else {
        Span::styled(
            &state.input,
            Style::default().fg(Color::Rgb(palette.text.r, palette.text.g, palette.text.b)),
        )
    };
    let input_paragraph = Paragraph::new(input_text)
        .block(input_block)
        .style(Style::default().bg(bg_color));
    f.render_widget(input_paragraph, chunks[0]);

    // Draw directory list with scrolling (bottom section with left, right, bottom borders)
    let list_inner_area = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::Rgb(
            palette.blue.r,
            palette.blue.g,
            palette.blue.b,
        )))
        .style(Style::default().bg(bg_color))
        .inner(chunks[1]);

    let list_block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::Rgb(
            palette.blue.r,
            palette.blue.g,
            palette.blue.b,
        )))
        .style(Style::default().bg(bg_color));
    f.render_widget(list_block, chunks[1]);

    // Calculate visible rows and update scroll offset
    let visible_rows = list_inner_area.height as usize;
    state.update_scroll(visible_rows);

    // Render visible directory items
    let start_idx = state.scroll_offset;
    let end_idx = (start_idx + visible_rows).min(state.filtered_dirs.len());

    for (row_idx, dir_idx) in (start_idx..end_idx).enumerate() {
        let path = &state.filtered_dirs[dir_idx];
        // Truncate path if it's too long
        let max_width = list_inner_area.width as usize;
        let path_str = crate::ui_utils::truncate_path_with_ellipsis(path, max_width);

        let is_selected = dir_idx == state.selected_index;
        let (fg, bg) = if is_selected {
            // Use the same highlight style as the main panel
            let highlight_bg =
                Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b);
            let highlight_fg = if !palette.is_dark {
                Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
            } else {
                Color::Rgb(palette.text.r, palette.text.g, palette.text.b)
            };
            (highlight_fg, highlight_bg)
        } else {
            (
                Color::Rgb(palette.text.r, palette.text.g, palette.text.b),
                bg_color,
            )
        };

        let line = Line::from(path_str).style(Style::default().fg(fg).bg(bg));
        f.render_widget(
            line,
            Rect {
                x: list_inner_area.x,
                y: list_inner_area.y + row_idx as u16,
                width: list_inner_area.width,
                height: 1,
            },
        );
    }

    // Draw scrollbar if needed
    let scroll_area = Rect {
        x: chunks[1].x + chunks[1].width - 1,
        y: chunks[1].y,
        width: 1,
        height: chunks[1].height.saturating_sub(1),
    };

    crate::ui_utils::draw_scrollbar(
        f,
        scroll_area,
        state.filtered_dirs.len(),
        visible_rows,
        state.selected_index,
        palette,
    );
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn test_fuzzy_search_truncation_integration() {
        // This test verifies that we can access and use the truncation logic
        // which is critical for the rendering code we just modified.
        let path = Path::new("/a/very/long/path/that/needs/truncation");
        let max_width = 20;
        let truncated = crate::ui_utils::truncate_path_with_ellipsis(path, max_width);
        
        assert!(truncated.len() <= max_width + 10); // Allow some buffer for unicode chars count vs len
        assert!(truncated.contains("…"));
    }
}
