use crate::app::FileViewerState;
use crate::theme::ThemePalette;
use crate::ui::ui_utils::TabScrollbarContext;
use lumis::highlight::Highlighter;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

/// Parse a hex color string like "#rrggbb" into a ratatui `Color::Rgb`.
fn parse_hex_color(hex: Option<&String>) -> Option<Color> {
    let hex = hex.as_ref()?;
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn normalize_selection(sel: ((usize, usize), (usize, usize))) -> ((usize, usize), (usize, usize)) {
    let ((r1, c1), (r2, c2)) = sel;
    if r1 < r2 || (r1 == r2 && c1 <= c2) {
        sel
    } else {
        ((r2, c2), (r1, c1))
    }
}

pub fn draw_file_viewer(
    f: &mut ratatui::Frame,
    viewer: &mut FileViewerState,
    area: Rect,
    palette: &ThemePalette,
    borders: bool,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(viewer.path.to_string_lossy())
        .border_style(Style::default().fg(if viewer.focused {
            Color::Rgb(palette.border.r, palette.border.g, palette.border.b)
        } else {
            Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
        }));

    let block = if borders {
        block.border_type(ratatui::widgets::BorderType::Rounded)
    } else {
        block.border_set(ratatui::symbols::border::EMPTY)
    };

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    if let Some(protocol) = &mut viewer.protocol
        && protocol.protocol_type().is_some()
    {
        f.render_stateful_widget(ratatui_image::StatefulImage::new(), inner_area, protocol);
        return;
    }

    if viewer.content.is_empty() {
        return;
    }

    let visible_lines = inner_area.height as usize;
    let max_lines = viewer.content.len();
    // Clamp scroll offset to valid range
    let start_line = viewer.scroll_offset.min(max_lines.saturating_sub(1));
    let end_line = (start_line + visible_lines).min(max_lines);

    let highlighter = Highlighter::new(viewer.language, viewer.theme.clone());
    let default_fg = viewer
        .theme
        .as_ref()
        .and_then(|t| t.fg().map(std::string::ToString::to_string))
        .and_then(|s| parse_hex_color(Some(&s)));

    let normalized_selection = viewer.selection.map(normalize_selection);

    let mut lines = Vec::new();
    for (i, line) in viewer.content[start_line..end_line].iter().enumerate() {
        let line_idx = start_line + i;
        let selection_range = normalized_selection.and_then(|((r1, c1), (r2, c2))| {
            if line_idx < r1 || line_idx > r2 {
                None
            } else if line_idx > r1 && line_idx < r2 {
                Some((0, usize::MAX))
            } else if r1 == r2 {
                Some((c1, c2))
            } else if line_idx == r1 {
                Some((c1, usize::MAX))
            } else {
                Some((0, c2))
            }
        });

        let segments = highlighter.highlight(line).unwrap_or_default();
        let ranges: Vec<(&lumis::themes::Style, &str)> = segments
            .iter()
            .map(|(style, text)| (style.as_ref(), *text))
            .collect();

        let spans = generate_line_spans(
            ranges,
            viewer.horizontal_scroll_offset,
            inner_area.width as usize,
            default_fg,
            selection_range,
            Some(Color::Rgb(
                palette.surface0.r,
                palette.surface0.g,
                palette.surface0.b,
            )),
        );

        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), inner_area);

    // Scrollbar
    let scroll_area = Rect {
        x: area.x + area.width - 1,
        y: area.y + 1,
        width: 1,
        height: area.height.saturating_sub(2),
    };

    crate::ui::ui_utils::draw_tab_scrollbar(
        f,
        scroll_area,
        max_lines,
        visible_lines,
        viewer.scroll_offset,
        &TabScrollbarContext {
            palette,
            borders,
            is_root: false,
            active: viewer.focused,
        },
    );
}

/// Generates spans for a single line, handling horizontal scrolling and width constraints
/// taking into account tab widths and wide characters.
#[must_use]
pub fn generate_line_spans(
    ranges: Vec<(&lumis::themes::Style, &str)>,
    h_offset: usize,
    max_width: usize,
    default_fg: Option<Color>,
    selection_char_range: Option<(usize, usize)>,
    selection_bg: Option<Color>,
) -> Vec<Span<'static>> {
    let mut display_pos = 0; // Current display column position
    let mut visible_width = 0; // Display width used so far
    let mut spans: Vec<Span> = Vec::new();
    let mut char_idx_counter = 0;

    for (style, text) in ranges {
        // Stop if we've already filled the available width
        if visible_width >= max_width {
            break;
        }

        // Calculate the TRUE display width of this segment, accounting for tabs
        let mut text_display_width = 0;
        let mut temp_pos = display_pos;
        for ch in text.chars() {
            let w = if ch == '\t' {
                4 - (temp_pos % 4)
            } else {
                unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
            };
            text_display_width += w;
            temp_pos += w;
        }

        let end_display_pos = display_pos + text_display_width;

        if end_display_pos > h_offset {
            // This segment is at least partially visible
            // We need to handle this character by character for tabs
            let mut current_display_pos = display_pos;

            for ch in text.chars() {
                let ch_width = if ch == '\t' {
                    // Tab width: advance to next multiple of 4
                    4 - (current_display_pos % 4)
                } else {
                    unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
                };

                let ch_end_pos = current_display_pos + ch_width;

                // Check if this character is visible
                if ch_end_pos > h_offset && current_display_pos < h_offset + max_width {
                    // Character is at least partially visible
                    let mut char_text = String::new();
                    let mut char_visible_width = 0;

                    if current_display_pos >= h_offset {
                        // Fully visible - check if we have room
                        if visible_width + ch_width <= max_width {
                            if ch == '\t' {
                                for _ in 0..ch_width {
                                    char_text.push(' ');
                                }
                            } else {
                                char_text.push(ch);
                            }
                            char_visible_width = ch_width;
                        } else {
                            // Would overflow - stop here
                            break;
                        }
                    } else {
                        // Partially visible (starts before h_offset)
                        if ch == '\t' {
                            let visible_tab_width = ch_end_pos - h_offset;
                            if visible_width + visible_tab_width <= max_width {
                                for _ in 0..visible_tab_width {
                                    char_text.push(' ');
                                }
                                char_visible_width = visible_tab_width;
                            }
                        }
                    }

                    if !char_text.is_empty() {
                        let color = parse_hex_color(Option::from(&style.fg)).or(default_fg);
                        let mut ratatui_style = if let Some(color) = color {
                            ratatui::style::Style::default().fg(color)
                        } else {
                            ratatui::style::Style::default()
                        };

                        // Apply selection background
                        if let Some((sel_start, sel_end)) = selection_char_range
                            && char_idx_counter >= sel_start
                            && char_idx_counter < sel_end
                        {
                            if let Some(bg) = selection_bg {
                                ratatui_style = ratatui_style.bg(bg);
                            } else {
                                ratatui_style = ratatui_style.bg(Color::Rgb(60, 60, 60)); // Fallback
                            }
                        }

                        // Try to merge with previous span if style is same
                        if let Some(last_span) = spans.last_mut()
                            && last_span.style == ratatui_style
                        {
                            let mut new_content = last_span.content.to_string();
                            new_content.push_str(&char_text);
                            last_span.content = new_content.into();
                        } else {
                            spans.push(Span::styled(char_text, ratatui_style));
                        }
                        visible_width += char_visible_width;
                        char_idx_counter += 1;
                    }
                } else {
                    char_idx_counter += 1;
                }

                current_display_pos = ch_end_pos;

                // Stop if we've filled the width
                if visible_width >= max_width {
                    break;
                }
            }
        }

        display_pos = end_display_pos;
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumis::themes::Style as LumisStyle;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn test_rendering_overflow_prevention() {
        // Simulate a viewport width
        let max_width = 10;
        let h_offset = 0;

        // Dummy style for testing
        let dummy_style = LumisStyle {
            fg: Some("#ffffff".to_string()),
            ..Default::default()
        };

        // Test cases that would overflow if tabs were counted as 1 char but rendered as 4 spaces
        // or if unicode width wasn't handled correctly.
        let test_cases = vec![
            // Case 1: Tabs expanding.
            // '\t' (4 spaces) + '\t' (4 spaces) + "ABC" (3 chars) = 11 width.
            // Char count = 5. If we only checked char count (5 < 10), this would overflow.
            ("\t\tABC", "Two tabs and text"),
            // Case 2: Mixed content just over the limit
            // "1234567890" (10 chars) + "1" = 11 width.
            ("12345678901", "Simple overflow"),
            // Case 3: Tab crossing the boundary
            // "12345678" (8 chars) + "\t" (4 chars -> pos 12).
            ("12345678\t", "Tab crossing boundary"),
        ];

        for (input, description) in test_cases {
            // Treat the whole line as one range for baseline testing
            let ranges = vec![(&dummy_style, input)];

            let spans = generate_line_spans(ranges, h_offset, max_width, None, None, None);

            // Calculate total display width of the generated spans
            let mut total_width = 0;
            let mut resulting_text = String::new();
            for span in &spans {
                // Note: generate_line_spans converts tabs to spaces, so width() works here
                total_width += span.content.width();
                resulting_text.push_str(&span.content);
            }

            println!("Test Case: {description}");
            println!("  Input: {input:?}");
            println!("  Result: '{resulting_text}'");
            println!("  Display width: {total_width}");

            // Assert that the total width does not exceed max_width
            assert!(
                total_width <= max_width,
                "Overflow detected for '{description}'! Width: {total_width}, Max: {max_width}. Result: '{resulting_text}'"
            );
        }
    }
}
