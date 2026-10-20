use crate::app_state::tabs::PanelSide;
use crate::app_state::tabs::Tab;
use crate::theme::ThemePalette;
use crate::ui::ui_utils::{is_root_user, panel_bg_color, truncate_middle_with_ellipsis};
use ratatui::prelude::*;
use ratatui::widgets::Block;

/// Context for rendering panel status bar
pub struct PanelStatusContext<'a> {
    pub palette: &'a ThemePalette,
    pub active: bool,
    pub borders: bool,
    pub task_manager: &'a crate::tasks::TaskManager,
    pub side: PanelSide,
}

/// Render the status bar for a panel, showing file counts, selection, and errors.
pub fn draw_panel_status(
    f: &mut ratatui::Frame,
    panel: &Tab,
    area: Rect,
    ctx: &PanelStatusContext,
) {
    // File filter mode: render the filter input in the status line
    if panel.filter.active {
        draw_file_filter_status(f, panel, area, ctx);
        return;
    }

    let error = panel.error.as_deref().unwrap_or("");
    // Count only files that are actually listed, so the totals do not include
    // entries withheld by the hidden-file setting.
    let file_count = panel.listed_file_count();
    let dir_count = panel
        .entries
        .iter()
        .enumerate()
        .filter(|(i, e)| e.is_dir && e.name != ".." && panel.is_visible(*i))
        .count();
    let selected_count = panel.entries.iter().filter(|e| e.selected).count();

    // Build the active-filter label (rendered in yellow, separated from the rest)
    let filter_label = panel.filter.applied.as_deref().map(|pattern| {
        format!(
            "Filter: {} {}/{} files, ",
            pattern,
            panel.visible_file_count(),
            file_count
        )
    });

    let status = if !error.is_empty() {
        error.to_string()
    } else if !panel.search.buffer.is_empty() {
        format!(
            "{} | {} matches",
            panel.search.buffer,
            panel.search.matching_indices.len()
        )
    } else {
        let items_info = if let Some((msg, instant)) = &panel.status_msg
            && instant.elapsed() < std::time::Duration::from_secs(3)
        {
            msg.clone()
        } else if panel.filter.is_active() {
            // File count is already shown in the filter label; only show dirs here.
            format!("{dir_count} dirs")
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
        if !panel.search.buffer.is_empty() {
            Color::Rgb(
                ctx.palette.yellow.r,
                ctx.palette.yellow.g,
                ctx.palette.yellow.b,
            )
        } else if let Some((_, instant)) = &panel.status_msg
            && instant.elapsed() < std::time::Duration::from_secs(3)
        {
            Color::Rgb(
                ctx.palette.yellow.r,
                ctx.palette.yellow.g,
                ctx.palette.yellow.b,
            )
        } else {
            Color::Rgb(ctx.palette.text.r, ctx.palette.text.g, ctx.palette.text.b)
        }
    } else {
        Color::Rgb(ctx.palette.red.r, ctx.palette.red.g, ctx.palette.red.b)
    };
    // Move status line one character to the right and reduce width by 2 (1 for left offset, 1 for right margin)
    let status_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };

    let is_root = is_root_user(panel);
    let panel_bg = panel_bg_color(ctx.palette, ctx.active, is_root, ctx.borders);

    // Clear the status area first to prevent artifacts
    f.render_widget(Block::default().style(Style::default().bg(panel_bg)), area);

    match ctx.side {
        PanelSide::Left => {
            draw_left_panel_status(
                f,
                ctx,
                status_area,
                status.as_ref(),
                fg,
                filter_label.as_deref(),
            );
        }
        PanelSide::Right => {
            draw_right_panel_status(
                f,
                ctx,
                status_area,
                status.as_ref(),
                fg,
                filter_label.as_deref(),
            );
        }
    }
}

fn draw_file_filter_status(
    f: &mut ratatui::Frame,
    panel: &Tab,
    area: Rect,
    ctx: &PanelStatusContext,
) {
    let is_root = is_root_user(panel);
    let panel_bg = panel_bg_color(ctx.palette, ctx.active, is_root, ctx.borders);

    // Clear the status area
    f.render_widget(Block::default().style(Style::default().bg(panel_bg)), area);

    let status_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };

    let yellow = Color::Rgb(
        ctx.palette.yellow.r,
        ctx.palette.yellow.g,
        ctx.palette.yellow.b,
    );

    let pattern = &panel.filter.pattern;
    let cursor_pos = panel.filter.cursor_position;

    // Check if pattern is a valid glob (non-empty pattern only)
    let error_msg = if pattern.trim().is_empty() {
        None
    } else {
        crate::state::compile_glob(&crate::state::effective_glob(pattern.trim()))
            .err()
            .map(|e| format!("{e}"))
    };

    let red = Color::Rgb(ctx.palette.red.r, ctx.palette.red.g, ctx.palette.red.b);

    // Build the input line: /pattern with a block cursor at cursor_position
    let prefix = Span::styled("/".to_string(), Style::default().fg(yellow));

    let before_cursor: String = pattern.chars().take(cursor_pos).collect();
    let cursor_char = pattern.chars().nth(cursor_pos);
    let after_cursor: String = pattern.chars().skip(cursor_pos + 1).collect();

    let fg = if error_msg.is_some() { red } else { yellow };

    let before_span = Span::styled(before_cursor, Style::default().fg(fg));
    let cursor_span = Span::styled(
        cursor_char.map_or("▎".to_string(), |c| c.to_string()),
        Style::default()
            .fg(fg)
            .add_modifier(ratatui::style::Modifier::REVERSED),
    );
    let after_span = Span::styled(after_cursor, Style::default().fg(fg));

    let mut spans = vec![prefix, before_span, cursor_span, after_span];

    if let Some(ref err) = error_msg {
        spans.push(Span::styled(format!("  {err}"), Style::default().fg(red)));
    }

    let line = ratatui::text::Line::from(spans);
    let paragraph = ratatui::widgets::Paragraph::new(line);
    f.render_widget(paragraph, status_area);
}

fn build_status_line(
    filter: Option<&str>,
    status: &str,
    fg: Color,
    yellow: Color,
) -> ratatui::text::Line<'static> {
    let filter_span = filter.map(|f| Span::styled(f.to_string(), Style::default().fg(yellow)));
    let status_span = Span::styled(status.to_string(), Style::default().fg(fg));

    let mut spans = Vec::new();
    if let Some(fs) = filter_span {
        spans.push(fs);
    }
    spans.push(status_span);
    ratatui::text::Line::from(spans)
}

fn draw_left_panel_status(
    f: &mut ratatui::Frame,
    ctx: &PanelStatusContext,
    status_area: Rect,
    status: &str,
    fg: Color,
    filter: Option<&str>,
) {
    let tasks = ctx.task_manager.get_tasks();
    let running_count = tasks
        .iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::Running))
        .count();
    let queued_count = tasks
        .iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::Queued))
        .count();

    let yellow = Color::Rgb(
        ctx.palette.yellow.r,
        ctx.palette.yellow.g,
        ctx.palette.yellow.b,
    );

    let activity_text = if running_count > 0 {
        Some(if queued_count == 0 {
            if running_count == 1 {
                "1 task running".to_string()
            } else {
                format!("{running_count} tasks running")
            }
        } else {
            format!("{running_count} running, {queued_count} queued")
        })
    } else if queued_count > 0 {
        Some(if queued_count == 1 {
            "1 task queued".to_string()
        } else {
            format!("{queued_count} tasks queued")
        })
    } else {
        None
    };

    if let Some(text) = activity_text {
        let text_width = u16::try_from(text.len()).unwrap_or(u16::MAX);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(text_width)])
            .split(status_area);

        // File Info (Left)
        let paragraph =
            ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow));
        f.render_widget(paragraph, chunks[0]);

        // Task Info (Right)
        let p = ratatui::widgets::Paragraph::new(text)
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::Rgb(
                ctx.palette.yellow.r,
                ctx.palette.yellow.g,
                ctx.palette.yellow.b,
            )));
        f.render_widget(p, chunks[1]);
    } else {
        // Check for recently completed/failed/cancelled tasks
        let last_finished = tasks
            .iter()
            .filter(|t| t.completed_at.is_some())
            .max_by_key(|t| t.completed_at);

        if let Some(t) = last_finished {
            let (text, task_fg) = match &t.status {
                crate::tasks::TaskStatus::Completed => {
                    (String::new(), ctx.palette.green) // no text for task completed status
                }
                crate::tasks::TaskStatus::Failed(e) if !t.error_reported => {
                    (format!("Task failed: {e}"), ctx.palette.red)
                }
                crate::tasks::TaskStatus::Failed(_) => {
                    // Already on a panel status line; don't say it twice.
                    (String::new(), ctx.palette.red)
                }
                crate::tasks::TaskStatus::Cancelled => {
                    ("Task cancelled".to_string(), ctx.palette.yellow)
                }
                crate::tasks::TaskStatus::Running | crate::tasks::TaskStatus::Queued => {
                    unreachable!()
                }
            };
            let text_width = u16::try_from(text.len()).unwrap_or(u16::MAX);

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(text_width)])
                .split(status_area);

            // File Info (Left)
            let paragraph =
                ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow));
            f.render_widget(paragraph, chunks[0]);

            // Task Info (Right)
            let p = ratatui::widgets::Paragraph::new(text)
                .alignment(Alignment::Right)
                .style(Style::default().fg(Color::Rgb(task_fg.r, task_fg.g, task_fg.b)));
            f.render_widget(p, chunks[1]);
        } else {
            // No tasks, just file info
            let paragraph =
                ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow));
            f.render_widget(paragraph, status_area);
        }
    }
}

fn draw_right_panel_status(
    f: &mut ratatui::Frame,
    ctx: &PanelStatusContext,
    status_area: Rect,
    status: &str,
    fg: Color,
    filter: Option<&str>,
) {
    // Right Panel: Progress on Left, Files/Dirs on Right
    // (Task results are only shown on the left panel status bar)

    let yellow = Color::Rgb(
        ctx.palette.yellow.r,
        ctx.palette.yellow.g,
        ctx.palette.yellow.b,
    );

    // Check for active task progress
    let tasks = ctx.task_manager.get_tasks();
    let active_task = tasks
        .iter()
        .rev()
        .find(|t| matches!(t.status, crate::tasks::TaskStatus::Running));

    if let Some(t) = active_task {
        // Calculate available width for progress info
        // We need to leave room for the status message on the right
        let status_width = u16::try_from(status.chars().count()).unwrap_or(u16::MAX);
        let spacing = 2; // Extra space between progress and status
        let available_progress_width = status_area.width.saturating_sub(status_width + spacing);

        let progress_spans = get_task_progress_spans(
            t.progress,
            t.byte_progress,
            t.rsync,
            t.current_file.as_deref(),
            available_progress_width as usize,
            ctx.palette,
        );

        let text_width = u16::try_from(
            progress_spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>(),
        )
        .unwrap_or(u16::MAX);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(text_width), Constraint::Min(0)])
            .split(status_area);

        // Task Info (Left)
        let progress_line = ratatui::text::Line::from(progress_spans);
        let progress_paragraph =
            ratatui::widgets::Paragraph::new(progress_line).alignment(Alignment::Left);
        f.render_widget(progress_paragraph, chunks[0]);

        // File Info (Right)
        let paragraph =
            ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow))
                .alignment(Alignment::Right);
        f.render_widget(paragraph, chunks[1]);
    } else {
        // Default: just file info (Right aligned)
        let paragraph =
            ratatui::widgets::Paragraph::new(build_status_line(filter, status, fg, yellow))
                .alignment(Alignment::Right);
        f.render_widget(paragraph, status_area);
    }
}

pub(super) fn get_task_progress_spans(
    progress: Option<(usize, usize)>,
    byte_progress: Option<(u64, u64)>,
    rsync: bool,
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
        let percent = (p * 100).checked_div(t).unwrap_or(0);
        let left = t.saturating_sub(p);
        item_progress_str = format!("{percent}% ({left} left) ");
    }

    let mut byte_progress_str = String::new();
    if let Some((p_bytes, t_bytes)) = byte_progress
        && t_bytes > 0
    {
        let percent = (p_bytes * 100)
            .checked_div(t_bytes)
            .map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX));
        byte_progress_str = format!(
            "[{}% of {}] ",
            percent,
            crate::fs::utils::format_size(Some(t_bytes), false, false).trim()
        );
    }

    let mut used_width = item_progress_str.chars().count() + byte_progress_str.chars().count();
    let rsync_str = if rsync { " [rsync] " } else { "" };
    used_width += rsync_str.chars().count();

    let mut separator = "";
    if !item_progress_str.is_empty() && current_file.is_some() {
        separator = "| ";
        used_width += separator.chars().count();
    }

    if !item_progress_str.is_empty() {
        progress_spans.push(Span::styled(item_progress_str, Style::default().fg(yellow)));
    }

    if !rsync_str.is_empty() {
        progress_spans.push(Span::styled(
            rsync_str,
            Style::default().fg(Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b)),
        ));
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
