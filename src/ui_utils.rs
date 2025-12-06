// Utility functions for UI truncation and formatting

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
    let mut result = format!("{}{}{}", left_str, ellipsis, right_str);
    let result_len = result.chars().count();
    if result_len > max_width {
        result = result.chars().take(max_width).collect();
    } else if result_len < max_width {
        result = format!("{:<width$}", result, width = max_width);
    }
    result
}

pub fn truncate_path_with_ellipsis(path: &std::path::Path, max_width: usize) -> String {
    use std::path::MAIN_SEPARATOR;
    let sep = MAIN_SEPARATOR;
    let path_str = path.to_string_lossy();
    let segments: Vec<&str> = path_str.split(sep).collect();
    if path_str.chars().count() <= max_width {
        return path_str.to_string();
    }
    let ellipsis = "…";
    let sep_str = sep.to_string();
    let seg_len = segments.len();
    let mut left_count = 1;
    let mut right_count = 1;
    let mut last_result = String::new();
    // Try all possible combinations, keep the best that fits
    while left_count + right_count < seg_len {
        let mut parts = Vec::new();
        if left_count > 0 {
            parts.extend_from_slice(&segments[..left_count.min(seg_len)]);
        }
        parts.push(ellipsis);
        if right_count > 0 {
            parts.extend_from_slice(&segments[seg_len.saturating_sub(right_count)..]);
        }
        let result = parts.join(&sep_str);
        if result.chars().count() > max_width {
            break;
        }
        last_result = result.clone();
        // Try to add more segments
        if left_count <= right_count {
            left_count += 1;
        } else {
            right_count += 1;
        }
    }
    // If nothing fit, fallback to first/ellipsis/last
    if last_result.is_empty() {
        let first = segments.first().map_or("", |v| *v);
        let last = segments.last().map_or("", |v| *v);
        last_result = format!("{}{}{}{}{}", first, sep_str, ellipsis, sep_str, last);
        if last_result.chars().count() > max_width {
            last_result = last_result.chars().take(max_width).collect();
        }
    }
    last_result
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
    fn test_truncate_path_with_ellipsis_linux() {
        let p = Path::new("/home/user/projects/very/deep/path");
        let s = truncate_path_with_ellipsis(p, 15);
        assert!(s.contains("…"));
        assert!(s.starts_with("/home"));
        assert!(s.ends_with("path"));
        assert!(s.chars().count() <= 15);
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

pub fn draw_scrollbar(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
    palette: &crate::theme::ThemePalette,
) {
    use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
    use ratatui::style::{Color, Style};

    if content_length > visible_length {
        let mut scrollbar_state = ScrollbarState::new(content_length)
            .viewport_content_length(visible_length)
            .position(offset);
        let scrollbar_color =
            Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .style(Style::default().fg(scrollbar_color));
        f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}
