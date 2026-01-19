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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_truncate_middle_with_ellipsis_short() {
        assert_eq!(truncate_middle_with_ellipsis("short.txt", 20), "short.txt");
    }

    #[test]
    fn test_truncate_middle_with_ellipsis_long() {
        let s = truncate_middle_with_ellipsis("verylongfilename.txt", 10);
        assert!(s.starts_with("very"));
        assert!(s.ends_with(".txt"));
        assert_eq!(s.chars().count(), 10);
    }

    #[test]
    fn test_truncate_middle_with_ellipsis_unicode() {
        let s = truncate_middle_with_ellipsis("αβγδεζηθικλμνξο.txt", 10);
        assert_eq!(s.chars().count(), 10);
    }

    #[test]
    fn test_truncate_path_with_ellipsis_long_path() {
        use std::path::PathBuf;
        let mut p = PathBuf::new();
        #[cfg(windows)]
        p.push("C:\\");
        #[cfg(not(windows))]
        p.push("/");

        p.push("home");
        p.push("user");
        p.push("projects");
        p.push("very");
        p.push("deep");
        p.push("path");

        let s = truncate_path_with_ellipsis(&p, 15);

        // Basic assertions
        assert!(s.contains("…"));
        assert!(s.chars().count() <= 15);
        assert!(s.ends_with("path"));

        // Platform-specific start assertion
        #[cfg(not(windows))]
        assert!(s.starts_with("/home"));
        #[cfg(windows)]
        assert!(s.starts_with("C:\\") && s.contains("…"));
    }

    #[test]
    fn test_truncate_path_with_ellipsis_unicode() {
        let p = Path::new("/α/β/γ/δε/ζηθικλμνξο/ω");
        let s = truncate_path_with_ellipsis(p, 12);
        assert!(s.contains("…"));
        assert!(s.chars().count() <= 12);
    }

    #[test]
    fn test_truncate_path_with_ellipsis_root() {
        let p = Path::new("/");
        let s = truncate_path_with_ellipsis(p, 5);
        assert_eq!(s, "/");
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

pub fn custom_border_set() -> ratatui::symbols::border::Set<'static> {
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
