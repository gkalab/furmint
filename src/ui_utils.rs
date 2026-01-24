// Utility functions for UI truncation, formatting, and generic UI widgets

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

/// Draws a simple horizontal row of button labels, à la yazi (hotkey style, no focus handling)
/// - `labels`: List of strings such as ["[Y]es", "(N)o"]
/// - `area`: Rect to render into
/// - `f`: Frame reference
/// - `text_color`: Foreground color for labels
pub fn draw_button_row(f: &mut ratatui::Frame<'_>, labels: &[&str], area: Rect, text_color: Color) {
    use ratatui::layout::Constraint;
    let constraints = vec![Constraint::Fill(1); labels.len()];
    let chunks = ratatui::layout::Layout::horizontal(constraints).split(area);
    for (i, label) in labels.iter().enumerate() {
        let p = Paragraph::new(*label)
            .alignment(Alignment::Center)
            .style(Style::default().fg(text_color).add_modifier(Modifier::BOLD));
        f.render_widget(p, chunks[i]);
    }
}

pub fn truncate_middle_with_ellipsis(name: &str, max_width: usize) -> String {
    let ellipsis = "…"; // Unicode ellipsis
    let ellipsis_len = ellipsis.chars().count();
    let name_len = name.chars().count();
    if name_len <= max_width {
        return name.to_string();
    }
    if max_width <= ellipsis_len {
        return ellipsis.repeat(max_width);
    }
    let keep = max_width - ellipsis_len;
    let left = keep / 2;
    let right = keep - left;
    let left_str: String = name.chars().take(left).collect();
    let right_str: String = name.chars().skip(name_len - right).collect();
    let mut result = format!("{left_str}{ellipsis}{right_str}");
    let result_len = result.chars().count();
    if result_len > max_width {
        result = result.chars().take(max_width).collect();
    } else if result_len < max_width {
        result = format!("{result:<max_width$}");
    }
    result
}

pub fn truncate_path_with_ellipsis(path: &std::path::Path, max_width: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.chars().count() <= max_width {
        return path_str.to_string();
    }

    let components: Vec<_> = path.components().collect();
    let total_components = components.len();

    if total_components == 0 {
        return String::new();
    }

    let ellipsis = "…";
    let mut left_count = 1;
    let mut right_count = 1;
    let mut last_result = String::new();

    // Iterate to find the best fit
    while left_count + right_count < total_components {
        let mut new_path = std::path::PathBuf::new();

        // Add left components
        for c in &components[..left_count] {
            new_path.push(c);
        }

        // Add ellipsis (as a component)
        new_path.push(ellipsis);

        // Add right components
        for c in &components[total_components.saturating_sub(right_count)..] {
            new_path.push(c);
        }

        let result = new_path.to_string_lossy().to_string();
        if result.chars().count() > max_width {
            break;
        }

        last_result = result;

        // Try to add more segments
        if left_count <= right_count {
            left_count += 1;
        } else {
            right_count += 1;
        }
    }

    // Fallback if nothing fits or initial split failed:
    // Truncate the whole string with ellipsis in the middle (using existing function)
    if last_result.is_empty() {
        // If we have components but couldn't fit even 1+1+ellipsis,
        // or if it was just 1 component that is too long.
        // fallback to string truncation
        return truncate_middle_with_ellipsis(&path_str, max_width);
    }

    last_result
}

pub fn draw_scrollbar(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
    palette: &crate::theme::ThemePalette,
    borders: bool,
) {
    use ratatui::style::{Color, Style};
    use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

    if content_length > visible_length {
        let mut scrollbar_state = ScrollbarState::new(content_length)
            .viewport_content_length(visible_length)
            .position(offset);
        let scrollbar_color =
            Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);
        let track_symbol = if borders { Some("│") } else { Some(" ") };
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(track_symbol)
            .thumb_symbol("▊")
            .style(Style::default().fg(scrollbar_color));
        f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

pub fn centered_rect_percent(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}

pub fn centered_rect_absolute(width: u16, height: u16, r: Rect) -> Rect {
    let popup_x = (r.width.saturating_sub(width)) / 2;
    let popup_y = (r.height.saturating_sub(height)) / 2;

    Rect {
        x: popup_x,
        y: popup_y,
        width,
        height,
    }
}

pub fn field_border_set() -> ratatui::symbols::border::Set<'static> {
    ratatui::symbols::border::Set {
        top_left: "▎",
        top_right: " ",
        bottom_left: "▎",
        bottom_right: " ",
        vertical_left: "▎",
        vertical_right: " ",
        horizontal_top: " ",
        horizontal_bottom: " ",
    }
}

pub fn message_border_set() -> ratatui::symbols::border::Set<'static> {
    ratatui::symbols::border::Set {
        top_left: "━",
        top_right: "━",
        bottom_left: " ",
        bottom_right: " ",
        vertical_left: " ",
        vertical_right: " ",
        horizontal_top: "━",
        horizontal_bottom: " ",
    }
}

pub struct InputPopupOptions<'a> {
    pub title: Option<&'a str>,
    pub input_value: &'a str,
    pub cursor_position: usize,
    pub error: Option<&'a str>,
    pub placeholder: &'a str,
    pub width: u16,
}

pub fn draw_input_popup(
    f: &mut ratatui::Frame,
    options: InputPopupOptions,
    palette: &crate::theme::ThemePalette,
) {
    use ratatui::widgets::{Block, Borders, Clear};

    let has_title = options.title.is_some();
    let popup_width = options.width;
    let popup_height = if has_title { 7 } else { 5 };
    let popup_area = centered_rect_absolute(popup_width, popup_height, f.area());

    // Clear the popup area
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let title_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let error_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let placeholder_color = Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);

    // Draw outer block (shadow/background)
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(ratatui::symbols::border::EMPTY)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    // Inner chunks for vertical spacing
    let constraints = if has_title {
        vec![Constraint::Length(5)]
    } else {
        vec![Constraint::Length(3)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints(constraints)
        .split(popup_area);

    // Draw input block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .border_set(field_border_set())
        .style(Style::default().bg(field_bg_color));

    f.render_widget(&block, chunks[0]);

    // Layout inside padding for title, input, error
    let text_area = Rect {
        x: chunks[0].x + 2,
        y: chunks[0].y,
        width: chunks[0].width.saturating_sub(2),
        height: chunks[0].height,
    };

    let layout_constraints = if has_title {
        vec![
            Constraint::Length(1), // Spacing (top border)
            Constraint::Length(1), // Title
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Input
            Constraint::Length(1), // Error
        ]
    } else {
        vec![
            Constraint::Length(1), // Spacing (top border)
            Constraint::Length(1), // Input
            Constraint::Length(1), // Error
        ]
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(layout_constraints)
        .split(text_area);

    if let Some(t) = options.title {
        let title_paragraph = Paragraph::new(t)
            .style(Style::default().fg(title_color).bg(field_bg_color))
            .alignment(Alignment::Left);
        f.render_widget(title_paragraph, layout[1]);
    }

    // Input text
    let input_index = if has_title { 3 } else { 1 };
    let input_width = (chunks[0].width as usize).saturating_sub(4);
    let cursor_pos = options.cursor_position;

    let scroll_offset = if cursor_pos < input_width {
        0
    } else {
        cursor_pos - input_width + 1
    };

    let display_text = if options.input_value.is_empty() {
        Span::styled(options.placeholder, Style::default().fg(placeholder_color))
    } else {
        let text: String = options
            .input_value
            .chars()
            .skip(scroll_offset)
            .take(input_width)
            .collect();
        Span::styled(text, Style::default().fg(text_color).bg(field_bg_color))
    };

    let paragraph = Paragraph::new(display_text).style(Style::default().bg(field_bg_color));

    f.render_widget(paragraph, layout[input_index]);

    // Cursor
    let cursor_visual_offset = cursor_pos.saturating_sub(scroll_offset);
    if cursor_visual_offset < input_width {
        let y_offset = if has_title { 3 } else { 1 };
        f.set_cursor_position(Position::new(
            chunks[0].x + 2 + cursor_visual_offset as u16,
            chunks[0].y + y_offset,
        ));
    }

    // Error
    let error_index = if has_title { 4 } else { 2 };
    if let Some(error) = options.error {
        let error_paragraph = Paragraph::new(error)
            .style(Style::default().fg(error_color).bg(field_bg_color))
            .alignment(Alignment::Right);
        f.render_widget(error_paragraph, layout[error_index]);
    }
}
