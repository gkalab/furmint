use crate::app::Tab;
use crate::app_state::tabs::{SortColumn, SortDirection};
use crate::fs::utils::{format_modified, format_size};
use crate::theme::ThemePalette;
use crate::ui_utils::truncate_middle_with_ellipsis;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use std::env;

/// Helper function to create a lighter shade of red for inactive borders
fn lighten_red(red: crate::theme::Rgb) -> crate::theme::Rgb {
    crate::theme::Rgb::new(
        ((red.r as u16 + 255) / 2) as u8,
        ((red.g as u16 + 255) / 2) as u8,
        ((red.b as u16 + 255) / 2) as u8,
    )
}

/// Check if the current tab is accessing a root location
fn is_root_user(tab: &Tab) -> bool {
    let system_user = env::var("USER").unwrap_or_default();
    tab.provider.display_prefix().starts_with("[root@")
        || (tab.provider.display_prefix().is_empty() && system_user == "root")
}

fn sort_indicator(
    column: SortColumn,
    current_column: SortColumn,
    direction: SortDirection,
) -> &'static str {
    if column == current_column {
        match direction {
            SortDirection::Ascending => "▴",
            SortDirection::Descending => "▾",
        }
    } else {
        ""
    }
}

/// Calculate the background color for a panel based on active state and root status
fn panel_bg_color(palette: &ThemePalette, active: bool, is_root: bool) -> Color {
    let base_bg = if active {
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    } else if palette.is_dark {
        let r = ((palette.base.r as u16 * 3 + palette.surface1.r as u16) / 4) as u8;
        let g = ((palette.base.g as u16 * 3 + palette.surface1.g as u16) / 4) as u8;
        let b = ((palette.base.b as u16 * 3 + palette.surface1.b as u16) / 4) as u8;
        Color::Rgb(r, g, b)
    } else {
        let r = ((palette.base.r as u16 * 14 + palette.surface1.r as u16) / 15) as u8;
        let g = ((palette.base.g as u16 * 14 + palette.surface1.g as u16) / 15) as u8;
        let b = ((palette.base.b as u16 * 14 + palette.surface1.b as u16) / 15) as u8;
        Color::Rgb(r, g, b)
    };

    if is_root {
        let (r0, g0, b0) = match base_bg {
            Color::Rgb(r, g, b) => (r as u16, g as u16, b as u16),
            _ => (
                palette.base.r as u16,
                palette.base.g as u16,
                palette.base.b as u16,
            ),
        };
        let r = ((r0 * 9 + palette.red.r as u16) / 10) as u8;
        let g = ((g0 * 9 + palette.red.g as u16) / 10) as u8;
        let b = ((b0 * 9 + palette.red.b as u16) / 10) as u8;
        Color::Rgb(r, g, b)
    } else {
        base_bg
    }
}

pub fn draw_panel(
    f: &mut ratatui::Frame,
    panel: &mut Tab,
    active: bool,
    area: Rect,
    palette: &ThemePalette,
    borders: bool,
) {
    // Calculate visible rows for scrolling logic
    let visible_rows = area.height.saturating_sub(3) as usize; // -2 for borders, -1 for header
    panel.scroll_to_cursor(visible_rows);

    // Compute max width for Size column
    let size_width = panel
        .entries
        .iter()
        .map(|e| format_size(e.size, e.is_dir, e.is_symlink).len())
        .max()
        .unwrap_or(4);
    let size_indicator = sort_indicator(SortColumn::Size, panel.sort_column, panel.sort_direction);
    let size_header_with_indicator = format!("Size{}", size_indicator);
    // Ensure column is wide enough for header + indicator
    let size_header = format!(
        "{:>width$}",
        size_header_with_indicator,
        width = size_width.max(size_header_with_indicator.len())
    );
    let name_indicator = sort_indicator(SortColumn::Name, panel.sort_column, panel.sort_direction);
    let name_header = format!("Name{}", name_indicator);
    let ext_indicator = sort_indicator(
        SortColumn::Extension,
        panel.sort_column,
        panel.sort_direction,
    );
    let name_header = if ext_indicator.is_empty() {
        name_header
    } else {
        format!("Name{}", ext_indicator)
    };
    let modified_header = format!(
        "Modified{}",
        sort_indicator(SortColumn::Date, panel.sort_column, panel.sort_direction)
    );
    let header = [
        name_header,
        size_header,
        modified_header,
        "Attributes".to_string(),
    ];

    let text_fg = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    // Calculate available width for name column
    let total_table_width = area.width as usize;
    let name_col_width = if total_table_width > (10 + 19 + 10) {
        total_table_width - (10 + 19 + 10)
    } else {
        10 // minimum width for name
    };

    let rows = panel
        .entries
        .iter()
        .enumerate()
        .skip(panel.scroll_offset)
        .take(visible_rows)
        .map(|(idx, e)| {
            // Account for ratatui border: subtract 2 from available width
            let visible_name_width = if name_col_width > 2 {
                name_col_width - 2
            } else {
                1
            };

            let name_style = if e.is_dir {
                Style::default().fg(Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b))
            } else if crate::fs::utils::is_executable(&panel.current_dir.join(&e.name), e) {
                Style::default().fg(Color::Rgb(
                    palette.green.r,
                    palette.green.g,
                    palette.green.b,
                ))
            } else {
                Style::default().fg(text_fg)
            };

            let name_cell = if let Some(matches) = panel.search_highlights.get(&idx) {
                let yellow = Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b);
                let highlight_style = Style::default().fg(yellow).add_modifier(Modifier::BOLD);

                let name_chars: Vec<char> = e.name.chars().collect();
                let name_len = name_chars.len();
                let mut spans = Vec::new();

                if name_len <= visible_name_width {
                    for (i, c) in name_chars.iter().enumerate() {
                        if matches.contains(&i) {
                            spans.push(Span::styled(c.to_string(), highlight_style));
                        } else {
                            spans.push(Span::raw(c.to_string()));
                        }
                    }
                } else {
                    // Truncate logic
                    let ellipsis = "…";
                    let ellipsis_len = 1;
                    let keep = visible_name_width.saturating_sub(ellipsis_len);
                    let left = keep / 2;
                    let right = keep - left;

                    // Left part
                    for (i, c) in name_chars.iter().take(left).enumerate() {
                        if matches.contains(&i) {
                            spans.push(Span::styled(c.to_string(), highlight_style));
                        } else {
                            spans.push(Span::raw(c.to_string()));
                        }
                    }
                    // Ellipsis
                    spans.push(Span::raw(ellipsis));
                    // Right part
                    let start_right = name_len.saturating_sub(right);
                    for (i, c) in name_chars.iter().skip(start_right).enumerate() {
                        let original_idx = start_right + i;
                        if matches.contains(&original_idx) {
                            spans.push(Span::styled(c.to_string(), highlight_style));
                        } else {
                            spans.push(Span::raw(c.to_string()));
                        }
                    }
                }
                Cell::from(Line::from(spans)).style(name_style)
            } else {
                let truncated_name = truncate_middle_with_ellipsis(&e.name, visible_name_width);
                Cell::from(truncated_name).style(name_style)
            };

            Row::new(vec![
                name_cell,
                Cell::from(format_size(e.size, e.is_dir, e.is_symlink))
                    .style(Style::default().fg(text_fg)),
                Cell::from(format_modified(e.modified)).style(Style::default().fg(text_fg)),
                Cell::from(e.attributes.clone()).style(Style::default().fg(text_fg)),
            ])
        });

    let is_root = is_root_user(panel);

    let border_color = if is_root && active {
        Color::Rgb(palette.red.r, palette.red.g, palette.red.b)
    } else if is_root && !active {
        let light_red = lighten_red(palette.red);
        Color::Rgb(light_red.r, light_red.g, light_red.b)
    } else if active {
        Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)
    } else {
        Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
    };

    let panel_bg = panel_bg_color(palette, active, is_root);

    let prefix = panel.provider.display_prefix();
    let path_str = panel.provider.display_path(&panel.current_dir);
    let full_title = if prefix.is_empty() {
        format!("{} ", path_str)
    } else {
        format!("{}:{} ", prefix, path_str)
    };
    let title_width = area.width.saturating_sub(4) as usize;
    let panel_title = if full_title.len() > title_width {
        crate::ui_utils::truncate_path_with_ellipsis(&panel.current_dir, title_width)
    } else {
        full_title
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title)
        .border_style(Style::default().fg(border_color).bg(panel_bg))
        .style(Style::default().bg(panel_bg));

    let block = if borders {
        block.border_type(ratatui::widgets::BorderType::Rounded)
    } else {
        block.border_set(ratatui::symbols::border::EMPTY)
    };
    let widths = [
        Constraint::Min(10),    // Name: dynamic, at least 10
        Constraint::Length(7),  // Size: always 7 (right-aligned)
        Constraint::Length(19), // Modified: always 19
        Constraint::Length(10), // Attributes: always 10
    ];
    let panel_selection_background = if active {
        Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b)
    } else {
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    };
    let panel_selection_foreground = if !palette.is_dark && active {
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
        .block(block);

    let table = if active {
        table.row_highlight_style(
            Style::default()
                .bg(panel_selection_background)
                .fg(panel_selection_foreground),
        )
    } else {
        table
    };

    f.render_stateful_widget(
        table,
        area,
        &mut TableState::default()
            .with_selected(Some(panel.cursor.saturating_sub(panel.scroll_offset))),
    );

    // Draw selection markers using half-block
    let yellow_color = Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b);

    for (idx, entry) in panel
        .entries
        .iter()
        .enumerate()
        .skip(panel.scroll_offset)
        .take(visible_rows)
    {
        let row_y = area.y + 2 + (idx - panel.scroll_offset) as u16; // +2 for border and header

        if row_y >= area.y + area.height - 1 {
            break;
        }

        if entry.selected {
            let marker = Span::styled("▊", Style::default().fg(yellow_color));
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
    }

    // Draw vertical scrollbar if needed
    let total_entries = panel.entries.len();

    let scroll_area = Rect {
        x: area.x + area.width - 1,
        y: area.y + 2, // +2 for border and header
        width: 1,
        height: visible_rows as u16,
    };

    crate::ui_utils::draw_scrollbar(
        f,
        scroll_area,
        total_entries,
        visible_rows,
        panel.cursor,
        palette,
        borders,
    );
}

pub fn draw_panel_status(
    f: &mut ratatui::Frame,
    panel: &Tab,
    area: Rect,
    palette: &ThemePalette,
    active: bool,
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
    } else if !panel.typed_buffer.is_empty() {
        format!(
            "{} | {} matches",
            panel.typed_buffer,
            panel.matching_indices.len()
        )
    } else {
        let items_info = if let Some((msg, instant)) = &panel.clipboard_msg
            && instant.elapsed() < std::time::Duration::from_secs(3)
        {
            msg.clone()
        } else {
            format!("{file_count} files, {dir_count} dirs")
        };

        if selected_count > 0 {
            format!("{items_info} | {selected_count} selected")
        } else {
            items_info
        }
    };

    // Use the same background as file/directory rows (surface1)
    let fg = if error.is_empty() {
        if !panel.typed_buffer.is_empty() {
            Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b)
        } else if let Some((_, instant)) = &panel.clipboard_msg
            && instant.elapsed() < std::time::Duration::from_secs(3)
        {
            Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b)
        } else {
            Color::Rgb(palette.text.r, palette.text.g, palette.text.b)
        }
    } else {
        Color::Rgb(palette.red.r, palette.red.g, palette.red.b)
    };
    // Move status line one character to the right and reduce width by 2 (1 for left offset, 1 for right margin)
    let status_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };

    let is_root = is_root_user(panel);
    let panel_bg = panel_bg_color(palette, active, is_root);

    // Clear the status area first to prevent artifacts
    f.render_widget(Block::default().style(Style::default().bg(panel_bg)), area);

    match side {
        crate::app::PanelSide::Left => {
            // Left Panel: Files/Dirs on Left, Running Tasks on Right
            let tasks = task_manager.get_tasks();
            let running_count = tasks
                .iter()
                .filter(|(_, _, s, _, _, _, _)| matches!(s, crate::tasks::TaskStatus::Running))
                .count();

            if running_count > 0 {
                let text = if running_count == 1 {
                    "1 task running".to_string()
                } else {
                    format!("{running_count} tasks running")
                };
                let text_width = text.len() as u16;

                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(0), Constraint::Length(text_width)])
                    .split(status_area);

                // File Info (Left)
                let paragraph =
                    ratatui::widgets::Paragraph::new(status).style(Style::default().fg(fg));
                f.render_widget(paragraph, chunks[0]);

                // Task Info (Right)
                let p = ratatui::widgets::Paragraph::new(text)
                    .alignment(Alignment::Right)
                    .style(Style::default().fg(Color::Rgb(
                        palette.yellow.r,
                        palette.yellow.g,
                        palette.yellow.b,
                    )));
                f.render_widget(p, chunks[1]);
            } else {
                // Check for recently completed/failed/cancelled tasks
                let last_finished = tasks
                    .iter()
                    .filter(|(_, _, _, _, _, _, completed_at)| completed_at.is_some())
                    .max_by_key(|(_, _, _, _, _, _, completed_at)| *completed_at);

                if let Some((_, _, status_task, _, _, _, _)) = last_finished {
                    let (text, task_fg) = match status_task {
                        crate::tasks::TaskStatus::Completed => {
                            ("".to_string(), palette.green) // no text for task completed status
                        }
                        crate::tasks::TaskStatus::Failed(e) => {
                            (format!("Task failed: {e}"), palette.red)
                        }
                        crate::tasks::TaskStatus::Cancelled => {
                            ("Task cancelled".to_string(), palette.yellow)
                        }
                        crate::tasks::TaskStatus::Running => unreachable!(),
                    };
                    let text_width = text.len() as u16;

                    let chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Min(0), Constraint::Length(text_width)])
                        .split(status_area);

                    // File Info (Left)
                    let paragraph =
                        ratatui::widgets::Paragraph::new(status).style(Style::default().fg(fg));
                    f.render_widget(paragraph, chunks[0]);

                    // Task Info (Right)
                    let p = ratatui::widgets::Paragraph::new(text)
                        .alignment(Alignment::Right)
                        .style(Style::default().fg(Color::Rgb(task_fg.r, task_fg.g, task_fg.b)));
                    f.render_widget(p, chunks[1]);
                } else {
                    // No tasks, just file info
                    let paragraph =
                        ratatui::widgets::Paragraph::new(status).style(Style::default().fg(fg));
                    f.render_widget(paragraph, status_area);
                }
            }
        }
        crate::app::PanelSide::Right => {
            // Right Panel: Progress on Left, Files/Dirs on Right
            // (Task results are only shown on the left panel status bar)

            // Check for active task progress
            let tasks = task_manager.get_tasks();
            let active_task = tasks
                .iter()
                .rev()
                .find(|t| matches!(t.2, crate::tasks::TaskStatus::Running));

            if let Some((
                _,
                _,
                crate::tasks::TaskStatus::Running,
                progress,
                byte_progress,
                current_file,
                _,
            )) = active_task
            {
                // Calculate available width for progress info
                // We need to leave room for the status message on the right
                let status_width = status.chars().count() as u16;
                let spacing = 2; // Extra space between progress and status
                let available_progress_width =
                    status_area.width.saturating_sub(status_width + spacing);

                let progress_spans = get_task_progress_spans(
                    progress.as_ref().copied(),
                    byte_progress.as_ref().copied(),
                    current_file.as_deref(),
                    available_progress_width as usize,
                    palette,
                );

                let text_width = progress_spans
                    .iter()
                    .map(|s| s.content.chars().count())
                    .sum::<usize>() as u16;

                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(text_width), Constraint::Min(0)])
                    .split(status_area);

                // Task Info (Left)
                let progress_line = Line::from(progress_spans);
                let progress_paragraph =
                    ratatui::widgets::Paragraph::new(progress_line).alignment(Alignment::Left);
                f.render_widget(progress_paragraph, chunks[0]);

                // File Info (Right)
                let paragraph = ratatui::widgets::Paragraph::new(status)
                    .alignment(Alignment::Right)
                    .style(Style::default().fg(fg));
                f.render_widget(paragraph, chunks[1]);
            } else {
                // Default: just file info (Right aligned)
                let paragraph = ratatui::widgets::Paragraph::new(status)
                    .alignment(Alignment::Right)
                    .style(Style::default().fg(fg));
                f.render_widget(paragraph, status_area);
            }
        }
    }
}

fn get_task_progress_spans(
    progress: Option<(usize, usize)>,
    byte_progress: Option<(u64, u64)>,
    current_file: Option<&str>,
    max_width: usize,
    palette: &ThemePalette,
) -> Vec<Span<'static>> {
    let mut progress_spans = Vec::new();
    let yellow = Color::Rgb(palette.yellow.r, palette.yellow.g, palette.yellow.b);

    let mut item_progress_str = String::new();
    if let Some((p, t)) = progress
        && t > 0
    {
        let percent = (p as f32 / t as f32 * 100.0) as usize;
        let left = t.saturating_sub(p);
        item_progress_str = format!("{}% ({} left) ", percent, left);
    }

    let mut byte_progress_str = String::new();
    if let Some((p_bytes, t_bytes)) = byte_progress
        && t_bytes > 0
    {
        let percent = (p_bytes as f32 / t_bytes as f32 * 100.0) as usize;
        byte_progress_str = format!(
            "[{}% of {}] ",
            percent,
            crate::fs::utils::format_size(Some(t_bytes), false, false).trim()
        );
    }

    let mut used_width = item_progress_str.chars().count() + byte_progress_str.chars().count();
    let mut separator = "";
    if !item_progress_str.is_empty() && current_file.is_some() {
        separator = "| ";
        used_width += separator.chars().count();
    }

    if !item_progress_str.is_empty() {
        progress_spans.push(Span::styled(item_progress_str, Style::default().fg(yellow)));
    }

    if let Some(file) = current_file {
        let text_fg = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
        if !separator.is_empty() {
            progress_spans.push(Span::styled(separator, Style::default().fg(text_fg)));
        }

        let available_for_file = max_width.saturating_sub(used_width);
        // Filename should take at least some space if possible, or be empty if no room at all
        if available_for_file > 0 {
            let truncated_file = format!(
                "{} ",
                truncate_middle_with_ellipsis(file, available_for_file.saturating_sub(1))
            );
            progress_spans.push(Span::styled(truncated_file, Style::default().fg(text_fg)));
        }
    }

    if !byte_progress_str.is_empty() {
        progress_spans.push(Span::styled(byte_progress_str, Style::default().fg(yellow)));
    }

    progress_spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_progress_truncation() {
        let palette = crate::theme::catppuccin_macchiato();

        // Case 1: No truncation needed
        // Progress strings: "50% (5 left) " (13) + "| " (2) + "[50% of 1000 B] " (~16) = ~31 overhead
        // "file.txt " = 9 chars. Total needed ~40.
        let max_width = 60;
        let spans = get_task_progress_spans(
            Some((5, 10)),
            Some((500, 1000)),
            Some("file.txt"),
            max_width,
            &palette,
        );
        let total_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(total_width <= max_width);
        assert!(spans.iter().any(|s| s.content.contains("file.txt")));

        // Case 2: Truncation needed for long filename
        // Overhead ~31. Max width 45. Available ~14.
        // File "very...txt" > 14. Should truncate.
        let case2_width = 45;
        let long_file = "very_long_filename_that_definitely_needs_truncation.txt";
        let spans = get_task_progress_spans(
            Some((5, 10)),
            Some((500, 1000)),
            Some(long_file),
            case2_width,
            &palette,
        );
        let total_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(total_width <= case2_width);
        // The filename should be truncated
        assert!(spans.iter().any(|s| s.content.contains("…")));

        // Case 3: Very narrow width
        // Max width 10. Overhead ~31. File should be dropped.
        // Total width will exceed max_width because progress bars are not truncated,
        // but we verify file is dropped.
        let narrow_width = 10;
        let spans = get_task_progress_spans(
            Some((5, 10)),
            Some((500, 1000)),
            Some("short.txt"),
            narrow_width,
            &palette,
        );
        // We expect the file to be absent
        assert!(!spans.iter().any(|s| s.content.contains("short.txt")));
    }
}
