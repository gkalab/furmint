use crate::tasks::TaskStatus;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

struct TaskDisplayData {
    id: usize,
    name: String,
    status: TaskStatus,
    progress: Option<(usize, usize)>,
    byte_progress: Option<(u64, u64)>,
    rsync: bool,
    current_file: Option<String>,
}

impl TaskDisplayData {
    pub fn new(
        id: usize,
        name: String,
        status: TaskStatus,
        progress: Option<(usize, usize)>,
        byte_progress: Option<(u64, u64)>,
        rsync: bool,
        current_file: Option<String>,
    ) -> Self {
        Self {
            id,
            name,
            status,
            progress,
            byte_progress,
            rsync,
            current_file,
        }
    }
}

struct TaskStyleContext<'a> {
    palette: &'a ThemePalette,
    text_color: Color,
    list_bg_color: Color,
    highlight_style: Style,
}

impl<'a> TaskStyleContext<'a> {
    pub fn new(
        palette: &'a ThemePalette,
        text_color: Color,
        list_bg_color: Color,
        highlight_style: Style,
    ) -> Self {
        Self {
            palette,
            text_color,
            list_bg_color,
            highlight_style,
        }
    }
}

pub fn draw_task_manager(
    f: &mut ratatui::Frame,
    task_manager: &crate::tasks::TaskManager,
    is_visible: bool,
    palette: &ThemePalette,
) {
    if !is_visible {
        return;
    }

    // Calculate popup size
    let popup_width = 80;
    let popup_height = 20;
    let popup_area =
        crate::ui::ui_utils::centered_rect_absolute(popup_width, popup_height, f.area());

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let list_bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(ratatui::symbols::border::EMPTY)
            .title("  Task Manager (Esc to close, x to cancel selected)  ")
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([Constraint::Min(1)])
        .split(popup_area)[0];

    let tasks = task_manager.get_tasks();

    // Use stateful list to show selection
    let selected_index = task_manager
        .selected_index
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut state = ListState::default();
    if !tasks.is_empty() {
        state.select(Some(selected_index));
    }

    let highlight_fg = if palette.is_dark {
        Color::Rgb(palette.text.r, palette.text.g, palette.text.b)
    } else {
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    };

    let highlight_style = Style::default()
        .bg(Color::Rgb(
            palette.surface2.r,
            palette.surface2.g,
            palette.surface2.b,
        ))
        .fg(highlight_fg)
        .add_modifier(Modifier::BOLD);

    let style_ctx = TaskStyleContext::new(palette, text_color, list_bg_color, highlight_style);

    let mut list_items = Vec::new();
    for (idx, (id, name, status, progress, byte_progress, rsync, current_file, _completed_at)) in
        tasks.into_iter().enumerate()
    {
        let data = TaskDisplayData::new(
            id,
            name,
            status,
            progress,
            byte_progress,
            rsync,
            current_file,
        );
        list_items.push(format_task_item(idx, selected_index, &data, &style_ctx));
    }

    // List Block inside
    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::EMPTY)
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(list_bg_color)),
        )
        .highlight_style(highlight_style);

    f.render_stateful_widget(list, inner_area, &mut state);
}

fn format_task_item<'a>(
    idx: usize,
    selected_index: usize,
    data: &TaskDisplayData,
    style_ctx: &'a TaskStyleContext<'a>,
) -> ListItem<'a> {
    let status_str = match &data.status {
        TaskStatus::Running => "Running",
        TaskStatus::Completed => "Completed",
        TaskStatus::Failed(_) => "Failed",
        TaskStatus::Cancelled => "Cancelled",
    };

    let style = if idx == selected_index {
        style_ctx.highlight_style
    } else {
        Style::default()
            .fg(style_ctx.text_color)
            .bg(style_ctx.list_bg_color)
    };

    // Task Name Line
    let rsync_indicator = if data.rsync { " [rsync]" } else { "" };
    let item_title = format!(
        "[{}] {}{} - {}",
        data.id, data.name, rsync_indicator, status_str
    );

    // Progress Bar Line
    let progress_line = if let Some((processed, total)) = data.progress {
        if total > 0 {
            let percentage = (processed * 100).checked_div(total).unwrap_or(0);
            let left = total.saturating_sub(processed);

            // Bar width: 20 chars
            let bar_width: usize = 20;
            let filled = (processed * bar_width).checked_div(total).unwrap_or(0);
            let empty = bar_width.saturating_sub(filled);

            let bar: String = "=".repeat(filled) + &" ".repeat(empty);

            format!("[{bar}] {percentage}% ({left} left)")
        } else {
            "Calculating...".to_string()
        }
    } else {
        String::new()
    };

    // Byte Progress Line
    let byte_progress_line = if let Some((processed, total)) = data.byte_progress {
        if total > 0 {
            let percentage = (processed * 100).checked_div(total).unwrap_or(0);

            // Format sizes
            let processed_str = crate::fs::utils::format_size(Some(processed), false, false)
                .trim()
                .to_string();
            let total_str = crate::fs::utils::format_size(Some(total), false, false)
                .trim()
                .to_string();

            format!("{percentage}% ({processed_str} / {total_str})")
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let mut spans = vec![Line::from(Span::styled(item_title, style))];

    if let Some(file) = &data.current_file {
        spans.push(Line::from(Span::styled(
            format!("Current: {file}"),
            style.fg(Color::Rgb(
                style_ctx.palette.yellow.r,
                style_ctx.palette.yellow.g,
                style_ctx.palette.yellow.b,
            )),
        )));
    }

    if !progress_line.is_empty() {
        spans.push(Line::from(Span::styled(progress_line, style)));
    }

    if !byte_progress_line.is_empty() {
        spans.push(Line::from(Span::styled(byte_progress_line, style)));
    }

    if let TaskStatus::Failed(e) = &data.status {
        spans.push(Line::from(Span::styled(
            format!("Error: {e}"),
            style.fg(Color::Rgb(
                style_ctx.palette.red.r,
                style_ctx.palette.red.g,
                style_ctx.palette.red.b,
            )),
        )));
    }

    ListItem::new(spans)
}
