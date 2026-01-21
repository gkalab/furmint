use crate::tasks::TaskStatus;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

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
    let popup_area = crate::ui_utils::centered_rect_absolute(popup_width, popup_height, f.area());

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let list_bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
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

    let mut list_items = Vec::new();
    for (idx, (id, name, status, progress, _completed_at)) in tasks.into_iter().enumerate() {
        let status_str = match status {
            TaskStatus::Running => "Running",
            TaskStatus::Completed => "Completed",
            TaskStatus::Failed(_) => "Failed",
            TaskStatus::Cancelled => "Cancelled",
        };

        let style = if idx == selected_index {
            highlight_style
        } else {
            Style::default().fg(text_color).bg(list_bg_color)
        };

        // Task Name Line
        let item_title = format!("[{id}] {name} - {status_str}");

        // Progress Bar Line
        let progress_line = if let Some((processed, total)) = progress {
            if total > 0 {
                let ratio = processed as f64 / total as f64;
                let percentage = (ratio * 100.0) as usize;
                let left = total.saturating_sub(processed);

                // Bar width: 20 chars
                let bar_width: usize = 20;
                let filled = (ratio * bar_width as f64).round() as usize;
                let empty = bar_width.saturating_sub(filled);

                let bar: String = "=".repeat(filled) + &" ".repeat(empty);

                format!("[{bar}] {percentage}% ({left} left)")
            } else {
                "Calculating...".to_string()
            }
        } else {
            String::new()
        };

        let mut spans = vec![Line::from(Span::styled(item_title, style))];

        if !progress_line.is_empty() {
            spans.push(Line::from(Span::styled(progress_line, style)));
        }

        if let TaskStatus::Failed(e) = status {
            spans.push(Line::from(Span::styled(
                format!("Error: {e}"),
                style.fg(Color::Rgb(palette.red.r, palette.red.g, palette.red.b)),
            )));
        }

        list_items.push(ListItem::new(spans));
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
