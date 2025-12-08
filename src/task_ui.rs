use crate::tasks::TaskStatus;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

pub fn draw_task_manager(
    f: &mut ratatui::Frame,
    task_manager: &crate::tasks::TaskManager,
    is_visible: bool,
    palette: &ThemePalette,
) {
    if !is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 80;
    let popup_height = 20;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title("Task Manager (Esc to close, x to cancel selected)")
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    f.render_widget(block.clone(), popup_area);

    let tasks = task_manager.get_tasks();

    let inner_area = block.inner(popup_area);

    // Use stateful list to show selection
    let selected_index = task_manager
        .selected_index
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut state = ListState::default();
    if !tasks.is_empty() {
        state.select(Some(selected_index));
    }

    let items: Vec<ListItem> = tasks
        .iter()
        .map(|(id, name, status, progress)| {
            let status_str = match status {
                TaskStatus::Running => "Running",
                TaskStatus::Completed => "Completed",
                TaskStatus::Failed(e) => {
                    return ListItem::new(format!("[{}] {} - Failed: {}", id, name, e)).style(
                        Style::default().fg(Color::Rgb(
                            palette.red.r,
                            palette.red.g,
                            palette.red.b,
                        )),
                    );
                }
                TaskStatus::Cancelled => "Cancelled",
            };

            let progress_str = if let Some(p) = progress {
                format!(" {:.1}%", p * 100.0)
            } else {
                "".to_string()
            };

            ListItem::new(format!(
                "[{}] {} - {}{}",
                id, name, status_str, progress_str
            ))
            .style(Style::default().fg(text_color))
        })
        .collect();

    let highlight_style = Style::default()
        .bg(Color::Rgb(
            palette.surface2.r,
            palette.surface2.g,
            palette.surface2.b,
        ))
        .fg(Color::Rgb(palette.base.r, palette.base.g, palette.base.b))
        .add_modifier(Modifier::BOLD);

    let list = List::new(items)
        .block(Block::default())
        .style(Style::default().bg(bg_color))
        .highlight_style(highlight_style);

    f.render_stateful_widget(list, inner_area, &mut state);
}

pub fn draw_task_status_bar(
    f: &mut ratatui::Frame,
    task_manager: &crate::tasks::TaskManager,
    area: Rect,
    palette: &ThemePalette,
) {
    let tasks = task_manager.get_tasks();
    let running_count = tasks
        .iter()
        .filter(|(_, _, s, _)| matches!(s, TaskStatus::Running))
        .count();

    if running_count > 0 {
        let text = format!("{} tasks running", running_count);
        let p = Paragraph::new(text).style(
            Style::default()
                .fg(Color::Rgb(
                    palette.yellow.r,
                    palette.yellow.g,
                    palette.yellow.b,
                ))
                .bg(Color::Rgb(palette.base.r, palette.base.g, palette.base.b)),
        ); // Use base bg to match status line
        f.render_widget(p, area);
    }
}
