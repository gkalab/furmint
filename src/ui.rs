use crate::app::{Tab, TabManager};
use crate::fs_ops::{format_modified, format_size};
use crate::theme::ThemePalette;
use crate::ui_utils::{truncate_middle_with_ellipsis, truncate_path_with_ellipsis};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

/// Draw the tab bar for a panel
pub fn draw_tab_bar(
    f: &mut ratatui::Frame,
    tab_manager: &TabManager,
    area: Rect,
    palette: &ThemePalette,
    active: bool,
) {
    if area.height == 0 {
        return;
    }

    let mut spans = Vec::new();
    let tab_count = tab_manager.tabs.len();

    for (idx, tab) in tab_manager.tabs.iter().enumerate() {
        // Get the last component of the path
        let tab_title = tab
            .current_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("/");

        // Truncate if too long (max 15 chars)
        let truncated_title = if tab_title.len() > 15 {
            format!("{}…", &tab_title[..12])
        } else {
            tab_title.to_string()
        };

        // Style based on whether this tab is active
        let is_active_tab = idx == tab_manager.active_tab_index;
        let (fg, bg) = if is_active_tab && active {
            // Active tab in active panel
            (
                Color::Rgb(palette.base.r, palette.base.g, palette.base.b),
                Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b),
            )
        } else if is_active_tab {
            // Active tab in inactive panel
            (
                Color::Rgb(palette.text.r, palette.text.g, palette.text.b),
                Color::Rgb(palette.surface1.r, palette.surface1.g, palette.surface1.b),
            )
        } else {
            // Inactive tab
            (
                Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b),
                Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b),
            )
        };

        // Add tab with padding
        spans.push(Span::styled(
            format!(" {} ", truncated_title),
            Style::default().fg(fg).bg(bg),
        ));

        // Add separator between tabs
        if idx < tab_count - 1 {
            spans.push(Span::raw(" "));
        }
    }

    let line = Line::from(spans);
    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let paragraph = ratatui::widgets::Paragraph::new(line).style(Style::default().bg(bg_color));
    f.render_widget(paragraph, area);
}

pub fn draw_panel(
    f: &mut ratatui::Frame,
    panel: &Tab,
    active: bool,
    area: Rect,
    palette: &ThemePalette,
) {
    // Compute max width for Size column
    let size_width = panel
        .entries
        .iter()
        .map(|e| format_size(e.size, e.is_dir, e.is_symlink).len())
        .max()
        .unwrap_or(4);
    let size_header = format!("{:>width$}", "Size", width = size_width);
    let header = ["Name", &size_header, "Modified", "Attributes"];

    let text_fg = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    // Calculate available width for name column
    let total_table_width = area.width as usize;
    let name_col_width = if total_table_width > (10 + 19 + 10) {
        total_table_width - (10 + 19 + 10)
    } else {
        10 // minimum width for name
    };

    let rows = panel.entries.iter().map(|e| {
        // Account for ratatui border: subtract 2 from available width
        let visible_name_width = if name_col_width > 2 {
            name_col_width - 2
        } else {
            1
        };
        let truncated_name = truncate_middle_with_ellipsis(&e.name, visible_name_width);
        let mut name_cell = Cell::from(truncated_name);
        let full_path = panel.current_dir.join(&e.name);
        let name_style = if e.is_dir {
            Style::default().fg(Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b))
        } else if is_executable(&full_path, e) {
            Style::default().fg(Color::Rgb(
                palette.green.r,
                palette.green.g,
                palette.green.b,
            ))
        } else {
            Style::default().fg(text_fg)
        };
        name_cell = name_cell.style(name_style);
        Row::new(vec![
            name_cell,
            Cell::from(format_size(e.size, e.is_dir, e.is_symlink))
                .style(Style::default().fg(text_fg)),
            Cell::from(format_modified(e.modified)).style(Style::default().fg(text_fg)),
            Cell::from(e.attributes.clone()).style(Style::default().fg(text_fg)),
        ])
    });

    // Helper to detect executables
    fn is_executable(full_path: &std::path::Path, e: &crate::fs_ops::FileEntry) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::symlink_metadata(full_path) {
                let mode = meta.permissions().mode();
                mode & 0o111 != 0 && !e.is_dir
            } else {
                false
            }
        }
        #[cfg(windows)]
        {
            let lower = e.name.to_lowercase();
            (lower.ends_with(".exe") || lower.ends_with(".bat") || lower.ends_with(".cmd"))
                && !e.is_dir
        }
    }

    let border_color = if active {
        Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)
    } else {
        Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
    };

    let title_width = area.width.saturating_sub(2) as usize;
    let panel_title = truncate_path_with_ellipsis(&panel.current_dir, title_width);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title)
        .border_style(Style::default().fg(border_color));
    let widths = [
        Constraint::Min(10),    // Name: dynamic, at least 10
        Constraint::Length(7),  // Size: always 7 (right-aligned)
        Constraint::Length(19), // Modified: always 19
        Constraint::Length(10), // Attributes: always 10
    ];
    let highlight_bg = if active {
        Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b)
    } else {
        // Use a lighter color for the selection line of the inactive panel
        Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b)
    };
    let highlight_fg = if !palette.is_dark && active {
        // For light themes, use the base background color for text on the dark selection background
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    } else {
        Color::Rgb(palette.text.r, palette.text.g, palette.text.b)
    };
    let table = Table::new(rows, widths)
        .header(Row::new(header).style(Style::default().fg(Color::Rgb(
            palette.yellow.r,
            palette.yellow.g,
            palette.yellow.b,
        ))))
        .block(block)
        .row_highlight_style(Style::default().bg(highlight_bg).fg(highlight_fg));
    f.render_stateful_widget(
        table,
        area,
        &mut TableState::default().with_selected(Some(panel.cursor)),
    );

    // Draw selection markers over the left border
    let yellow_color = Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b);
    let border_color = if active {
        Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)
    } else {
        Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
    };

    // Calculate which entries are visible in the current view
    let visible_rows = area.height.saturating_sub(2) as usize; // -2 for top/bottom borders
    let start_idx = if panel.cursor >= visible_rows {
        panel.cursor - visible_rows + 1
    } else {
        0
    };

    for (idx, entry) in panel
        .entries
        .iter()
        .enumerate()
        .skip(start_idx)
        .take(visible_rows)
    {
        let row_y = area.y + 2 + (idx - start_idx) as u16; // +2 for border and header

        if row_y >= area.y + area.height - 1 {
            break; // Don't draw past the bottom border
        }

        let marker = if entry.selected {
            Span::styled("█", Style::default().fg(yellow_color))
        } else {
            // Restore the border character
            Span::styled("│", Style::default().fg(border_color))
        };

        f.render_widget(
            Line::from(marker),
            Rect {
                x: area.x,
                y: row_y,
                width: 1,
                height: 1,
            },
        );
    }

    // Draw unobtrusive vertical scrollbar if needed
    let visible_rows = area.height.saturating_sub(1) as usize; // 1 for header
    let total_entries = panel.entries.len();

    let scroll_area = Rect {
        x: area.x + area.width - 1,
        y: area.y + 2, // +2 for border and header
        width: 1,
        height: (visible_rows as u16).saturating_sub(2),
    };

    crate::ui_utils::draw_scrollbar(
        f,
        scroll_area,
        total_entries,
        visible_rows,
        panel.cursor,
        palette,
    );
}

pub fn draw_panel_status(
    f: &mut ratatui::Frame,
    panel: &Tab,
    area: Rect,
    palette: &ThemePalette,
    _active: bool,
    task_manager: &crate::tasks::TaskManager,
    side: crate::app::PanelSide,
) {
    let error = panel.error.as_deref().unwrap_or("");
    let file_count = panel.entries.iter().filter(|e| !e.is_dir).count();
    let dir_count = panel
        .entries
        .iter()
        .filter(|e| e.is_dir && e.name != "..")
        .count();
    let selected_count = panel.entries.iter().filter(|e| e.selected).count();

    let status = if !error.is_empty() {
        error.to_string()
    } else if selected_count > 0 {
        format!(
            "{} files, {} dirs | {} selected",
            file_count, dir_count, selected_count
        )
    } else {
        format!("{} files, {} dirs", file_count, dir_count)
    };
    // Use the same background as file/directory rows (surface1)
    let fg = if !error.is_empty() {
        Color::Rgb(palette.red.r, palette.red.g, palette.red.b)
    } else {
        Color::Rgb(palette.text.r, palette.text.g, palette.text.b)
    };
    // Move status line one character to the right and reduce width by 2 (1 for left offset, 1 for right margin)
    let status_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };

    // calculate bg color for clearing
    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);

    // Clear the status area first to prevent artifacts
    f.render_widget(Block::default().style(Style::default().bg(bg_color)), status_area);

    match side {
        crate::app::PanelSide::Left => {
            // Left Panel: Files/Dirs on Left, Running Tasks on Right
            let running_count = task_manager.get_tasks()
                .iter()
                .filter(|(_, _, s, _)| matches!(s, crate::tasks::TaskStatus::Running))
                .count();

            if running_count > 0 {
                let text = if running_count == 1 {
                    "1 task running".to_string()
                } else {
                    format!("{} tasks running", running_count)
                };
                let text_width = text.len() as u16;
                
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Min(0),
                        Constraint::Length(text_width),
                    ])
                    .split(status_area);

                // File Info (Left)
                let paragraph = ratatui::widgets::Paragraph::new(status).style(Style::default().fg(fg));
                f.render_widget(paragraph, chunks[0]);

                // Task Info (Right)
                let p = ratatui::widgets::Paragraph::new(text)
                    .alignment(Alignment::Right)
                    .style(Style::default().fg(Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b)));
                f.render_widget(p, chunks[1]);
            } else {
                // No tasks, just file info
                let paragraph = ratatui::widgets::Paragraph::new(status).style(Style::default().fg(fg));
                f.render_widget(paragraph, status_area);
            }
        }
        crate::app::PanelSide::Right => {
            // Right Panel: Progress on Left, Files/Dirs on Right
            
            // Check for active task progress
            let tasks = task_manager.get_tasks();
            let active_task = tasks.iter().filter(|t| matches!(t.2, crate::tasks::TaskStatus::Running)).last();

            if let Some((_, _, crate::tasks::TaskStatus::Running, Some((processed, total)))) = active_task {
                if *total > 0 {
                     let percent = (*processed as f32 / *total as f32 * 100.0) as usize;
                     let progress_text = format!("{}% ({} left)", percent, total.saturating_sub(*processed));
                     let text_width = progress_text.len() as u16 + 2; // Add some spacing
                     
                     let chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Length(text_width),
                            Constraint::Min(0),
                        ])
                        .split(status_area);

                     // Task Info (Left)
                     let progress_paragraph = ratatui::widgets::Paragraph::new(progress_text)
                         .alignment(Alignment::Left)
                         .style(Style::default().fg(Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b)));
                     f.render_widget(progress_paragraph, chunks[0]);

                     // File Info (Right)
                     let paragraph = ratatui::widgets::Paragraph::new(status)
                        .alignment(Alignment::Right)
                        .style(Style::default().fg(fg));
                     f.render_widget(paragraph, chunks[1]);
                     return;
                }
            }
            
            // Default: just file info (Right aligned)
            let paragraph = ratatui::widgets::Paragraph::new(status)
                .alignment(Alignment::Right)
                .style(Style::default().fg(fg));
            f.render_widget(paragraph, status_area);
        }
    }
}

pub fn draw_file_viewer(
    f: &mut ratatui::Frame,
    viewer: &crate::app::FileViewerState,
    area: Rect,
    palette: &ThemePalette,
) {
    use syntect::easy::HighlightLines;

    let block = Block::default()
        .borders(Borders::ALL)
        .title(viewer.path.to_string_lossy())
        .border_style(Style::default().fg(if viewer.focused {
            Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)
        } else {
            Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
        }));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

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

    for (i, line) in viewer.content[start_line..end_line].iter().enumerate() {
        let ranges: Vec<(syntect::highlighting::Style, &str)> = h
            .highlight_line(line, &viewer.syntax_set)
            .unwrap_or_default();

        let spans = generate_line_spans(
            ranges,
            viewer.horizontal_scroll_offset,
            inner_area.width as usize,
        );

        f.render_widget(
            Line::from(spans),
            Rect {
                x: inner_area.x,
                y: inner_area.y + i as u16,
                width: inner_area.width,
                height: 1,
            },
        );
    }

    // Scrollbar
    let scroll_area = Rect {
        x: area.x + area.width - 1,
        y: area.y + 1,
        width: 1,
        height: area.height.saturating_sub(2),
    };

    crate::ui_utils::draw_scrollbar(
        f,
        scroll_area,
        max_lines,
        visible_lines,
        viewer.scroll_offset,
        palette,
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

            println!("Test Case: {}", description);
            println!("  Input: {:?}", input);
            println!("  Result: '{}'", resulting_text);
            println!("  Display width: {}", total_width);

            // Assert that the total width does not exceed max_width
            assert!(
                total_width <= max_width,
                "Overflow detected for '{}'! Width: {}, Max: {}. Result: '{}'",
                description,
                total_width,
                max_width,
                resulting_text
            );
        }
    }
}
