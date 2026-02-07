use crate::app::FileViewerState;
use crate::theme::ThemePalette;
use crate::ui::ui_utils::TabScrollbarContext;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use syntect::easy::HighlightLines;

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

    // Use cached syntax name
    let syntax = viewer
        .syntax_name
        .as_ref()
        .and_then(|name| viewer.syntax_set.find_syntax_by_name(name))
        .unwrap_or_else(|| viewer.syntax_set.find_syntax_plain_text());

    let mut h = HighlightLines::new(syntax, &viewer.theme);

    let visible_lines = inner_area.height as usize;
    let max_lines = viewer.content.len();
    // Clamp scroll offset to valid range
    let start_line = viewer.scroll_offset.min(max_lines.saturating_sub(1));
    let end_line = (start_line + visible_lines).min(max_lines);

    let mut lines = Vec::new();
    for line in viewer.content[start_line..end_line].iter() {
        let ranges = h
            .highlight_line(line, &viewer.syntax_set)
            .unwrap_or_default();

        let spans = generate_line_spans(
            ranges,
            viewer.horizontal_scroll_offset,
            inner_area.width as usize,
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
pub fn generate_line_spans(
    ranges: Vec<(syntect::highlighting::Style, &str)>,
    h_offset: usize,
    max_width: usize,
) -> Vec<Span<'static>> {
    let mut display_pos = 0; // Current display column position
    let mut visible_width = 0; // Display width used so far
    let mut spans: Vec<Span> = Vec::new();

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
            let mut result_text = String::new();
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
                    if current_display_pos >= h_offset {
                        // Fully visible - check if we have room
                        if visible_width + ch_width <= max_width {
                            // If it's a tab, we should probably render spaces to be safe and consistent
                            // especially since we're calculating width based on spaces
                            if ch == '\t' {
                                for _ in 0..ch_width {
                                    result_text.push(' ');
                                }
                            } else {
                                result_text.push(ch);
                            }
                            visible_width += ch_width;
                        } else {
                            // Would overflow - stop here
                            break;
                        }
                    } else {
                        // Partially visible (starts before h_offset)
                        // For tabs, we need to show spaces for the visible portion
                        if ch == '\t' {
                            let visible_tab_width = ch_end_pos - h_offset;
                            if visible_width + visible_tab_width <= max_width {
                                // Show spaces for the visible part of the tab
                                for _ in 0..visible_tab_width {
                                    result_text.push(' ');
                                }
                                visible_width += visible_tab_width;
                            }
                        } else {
                            // Regular character partially scrolled off - skip it
                            // (we can't show half a character)
                        }
                    }
                }

                current_display_pos = ch_end_pos;

                // Stop if we've filled the width
                if visible_width >= max_width {
                    break;
                }
            }

            if !result_text.is_empty() {
                let fg = style.foreground;
                spans.push(Span::styled(
                    result_text,
                    Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b)),
                ));
            }
        }

        display_pos = end_display_pos;
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntect::highlighting::{Color, FontStyle, Style};
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn test_rendering_overflow_prevention() {
        // Simulate a viewport width
        let max_width = 10;
        let h_offset = 0;

        // Dummy style for testing
        let dummy_style = Style {
            foreground: Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            background: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            font_style: FontStyle::empty(),
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
            let ranges = vec![(dummy_style, input)];

            let spans = generate_line_spans(ranges, h_offset, max_width);

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
