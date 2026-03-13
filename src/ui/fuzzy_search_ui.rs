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
    #[must_use]
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

    #[must_use]
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
    let popup_width = u16::try_from((u32::from(area.width) * 6 / 10).min(80)).unwrap_or(area.width);
    let popup_height =
        u16::try_from((u32::from(area.height) * 5 / 10).min(20)).unwrap_or(area.height);
    let popup_area = crate::ui::ui_utils::centered_rect_absolute(popup_width, popup_height, area);

    // Clear the popup area first
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);

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

    draw_input_box(f, state, palette, chunks[0], border_color, field_bg_color);

    // Draw directory list
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::EMPTY)
        .border_style(Style::default().fg(border_color).bg(bg_color))
        .style(Style::default().bg(bg_color));

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
        let path_str = crate::ui::ui_utils::truncate_path_with_ellipsis(path, max_width);

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
                bg_color,
            )
        };

        let line = Line::from(path_str).style(Style::default().fg(fg).bg(bg));
        f.render_widget(
            line,
            Rect {
                x: list_inner_area.x,
                y: list_inner_area.y + u16::try_from(row_idx).unwrap_or(0),
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

    crate::ui::ui_utils::draw_scrollbar(
        f,
        scroll_area,
        state.filtered_dirs.len(),
        visible_rows,
        state.selected_index,
        palette,
    );
}

fn draw_input_box(
    f: &mut ratatui::Frame,
    state: &FuzzySearchState,
    palette: &ThemePalette,
    area: ratatui::layout::Rect,
    border_color: ratatui::style::Color,
    field_bg_color: ratatui::style::Color,
) {
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_set(crate::ui::ui_utils::field_border_set())
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .style(Style::default().bg(field_bg_color));

    f.render_widget(input_block.clone(), area);
    let input_inner = input_block.inner(area);
    let input_width = (input_inner.width as usize).saturating_sub(2);

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
        ratatui::text::Span::styled(
            "Type to search...",
            Style::default().fg(Color::Rgb(
                palette.overlay0.r,
                palette.overlay0.g,
                palette.overlay0.b,
            )),
        )
    } else {
        ratatui::text::Span::styled(
            &display_input,
            Style::default().fg(Color::Rgb(palette.text.r, palette.text.g, palette.text.b)),
        )
    };

    let input_paragraph = Paragraph::new(input_text).style(Style::default().bg(field_bg_color));

    let text_area = ratatui::layout::Rect {
        x: input_inner.x + 1,
        y: input_inner.y,
        width: input_inner.width.saturating_sub(2),
        height: input_inner.height,
    };
    f.render_widget(input_paragraph, text_area);

    let cursor_visual_offset = cursor_pos.saturating_sub(input_scroll_offset);
    if cursor_visual_offset < input_width {
        f.set_cursor_position(ratatui::layout::Position::new(
            area.x + 2 + u16::try_from(cursor_visual_offset).unwrap_or(0),
            area.y + 1,
        ));
    }
}
