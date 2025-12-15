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

    let highlight_style = Style::default()
        .bg(Color::Rgb(
            palette.surface2.r,
            palette.surface2.g,
            palette.surface2.b,
        ))
        .fg(Color::Rgb(palette.base.r, palette.base.g, palette.base.b))
        .add_modifier(Modifier::BOLD);

    let mut list_items = Vec::new();
    for (idx, (id, name, status, progress)) in tasks.into_iter().enumerate() {
        let status_str = match status {
            TaskStatus::Running => "Running",
            TaskStatus::Completed => "Completed",
            TaskStatus::Failed(_) => "Failed",
            TaskStatus::Cancelled => "Cancelled",
        };

        let style = if idx == selected_index {
            highlight_style
        } else {
            Style::default().fg(text_color)
        };

        // Task Name Line
        let item_title = format!("[{}] {} - {}", id, name, status_str);

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

                let bar: String = std::iter::repeat('=').take(filled).collect::<String>()
                    + &std::iter::repeat(' ').take(empty).collect::<String>();
                
                format!("[{}] {}% ({} left)", bar, percentage, left)
            } else {
                "Calculating...".to_string()
            }
        } else {
            String::new()
        };

        // If failed, append error to title or a separate line?
        // Let's just keep title simple for now. 
        // We create a Multi-line item? List items are usually single line? 
        // Ratatui List items can be multi-line if they contain newlines? 
        // No, ListItem takes a generic Text which can be lines.
        
        let mut spans = vec![
            Line::from(Span::styled(item_title, style)),
        ];

        if !progress_line.is_empty() {
             spans.push(Line::from(Span::styled(progress_line, style)));
        }
        
        if let TaskStatus::Failed(e) = status {
             spans.push(Line::from(Span::styled(format!("Error: {}", e), style.fg(Color::Rgb(palette.red.r, palette.red.g, palette.red.b)))));
        }

        // Add a separator or just spacing?
        // Usually list items are compact.
        
        list_items.push(ListItem::new(spans));
    }

    let list = List::new(list_items)
        .block(Block::default())
        .style(Style::default().bg(bg_color))
        .highlight_style(highlight_style);

    f.render_stateful_widget(list, inner_area, &mut state);
}



