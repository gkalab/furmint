use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::path::PathBuf;

#[derive(Default)]
pub struct FuzzySearchState {
    pub is_visible: bool,
    pub input: String,
    pub cursor_position: usize,
    pub filtered_dirs: Vec<PathBuf>,
    pub selected_index: usize,
    pub scroll_offset: usize,
}

impl FuzzySearchState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            input: String::new(),
            cursor_position: 0,
            filtered_dirs: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
        }
    }

    pub fn reset(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
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

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.input.len();
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
    let popup_width = (f32::from(area.width) * 0.6).min(80.0) as u16;
    let popup_height = (f32::from(area.height) * 0.5).min(20.0) as u16;
    let popup_area = crate::ui_utils::centered_rect_absolute(popup_width, popup_height, area);

    // Clear the popup area first
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(ratatui::symbols::border::EMPTY)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    // Split popup into input area and list area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([
            Constraint::Length(3), // Input box
            Constraint::Length(1), // Gap
            Constraint::Min(1),    // Directory list
        ])
        .split(popup_area);

    // Draw input box
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_set(crate::ui_utils::custom_border_set())
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .style(Style::default().bg(field_bg_color));

    f.render_widget(input_block.clone(), chunks[0]);
    let input_inner = input_block.inner(chunks[0]);
    let input_width = (input_inner.width as usize).saturating_sub(2);

    // Calculate input scroll offset based on cursor position
    let cursor_pos = state.cursor_position;
    let input_scroll_offset = if cursor_pos < input_width {
        0
    } else {
        cursor_pos - input_width + 1
    };

    let display_input: String = if state.input.is_empty() {
        "Type to search...".to_string()
    } else {
        state
            .input
            .chars()
            .skip(input_scroll_offset)
            .take(input_width)
            .collect()
    };

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
            &display_input,
            Style::default().fg(Color::Rgb(palette.text.r, palette.text.g, palette.text.b)),
        )
    };

    let input_paragraph = Paragraph::new(input_text).style(Style::default().bg(field_bg_color));

    // Adjust input text area (padding)
    let text_area = Rect {
        x: input_inner.x + 1,
        y: input_inner.y,
        width: input_inner.width.saturating_sub(2),
        height: input_inner.height,
    };
    f.render_widget(input_paragraph, text_area);

    // Render cursor
    let cursor_visual_offset = cursor_pos.saturating_sub(input_scroll_offset);
    if cursor_visual_offset < input_width {
        f.set_cursor_position(Position::new(
            chunks[0].x + 2 + cursor_visual_offset as u16,
            chunks[0].y + 1,
        ));
    }

    // Draw directory list
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::EMPTY)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .style(Style::default().bg(field_bg_color));

    f.render_widget(list_block.clone(), chunks[2]);
    let list_inner_area = list_block.inner(chunks[2]);

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
            let list_selection_background =
                Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b);
            let list_selection_foreground = if palette.is_dark {
                Color::Rgb(palette.text.r, palette.text.g, palette.text.b)
            } else {
                Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
            };
            (list_selection_foreground, list_selection_background)
        } else {
            (
                Color::Rgb(palette.text.r, palette.text.g, palette.text.b),
                field_bg_color,
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
    // Using list_inner_area right edge.
    let scroll_area = Rect {
        x: list_inner_area.x + list_inner_area.width.saturating_sub(1) + 1,
        y: list_inner_area.y,
        width: 1,
        height: list_inner_area.height,
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
