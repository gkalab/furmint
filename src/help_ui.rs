use crate::config::KeyboardConfig;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Clear, Row, Table};

pub fn draw_help_popup(
    f: &mut ratatui::Frame,
    is_visible: bool,
    keyboard: &KeyboardConfig,
    palette: &ThemePalette,
) {
    if !is_visible {
        return;
    }

    let size = f.area();
    let area = centered_rect(80, 80, size);

    let blue = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let base = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let text = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    // Dynamic grouping of key bindings
    let categories = vec![
        (
            "Navigation",
            vec![
                ("Back", &keyboard.back),
                ("Forward", &keyboard.forward),
                ("Enter Dir", &keyboard.enter_dir),
                ("Up Dir", &keyboard.up_dir),
                ("New Tab", &keyboard.new_tab),
                ("Next Tab", &keyboard.tab_next),
                ("Prev Tab", &keyboard.tab_prev),
                ("Close Tab", &keyboard.tab_close),
            ],
        ),
        (
            "File Operations",
            vec![
                ("New File", &keyboard.new_file),
                ("New Dir", &keyboard.new_dir),
                ("Edit", &keyboard.edit_file),
                ("Copy", &keyboard.copy_to),
                ("Move", &keyboard.move_to),
                ("Rename", &keyboard.rename),
                ("Delete", &keyboard.delete),
                ("Force Delete", &keyboard.delete_force),
                ("Empty Trash", &keyboard.empty_trash),
            ],
        ),
        (
            "Sorting",
            vec![
                ("Sort by Name", &keyboard.sort_name),
                ("Sort by Ext", &keyboard.sort_ext),
                ("Sort by Date", &keyboard.sort_date),
                ("Sort by Size", &keyboard.sort_size),
            ],
        ),
        (
            "App",
            vec![
                ("Help", &keyboard.help),
                ("Tasks", &keyboard.tasks),
                ("Terminal", &keyboard.open_terminal),
                ("Quit", &keyboard.quit),
            ],
        ),
    ];

    let mut rows = Vec::new();
    for (category, bindings) in categories {
        rows.push(Row::new(vec![Cell::from(Span::styled(
            category,
            Style::default().add_modifier(Modifier::BOLD).fg(blue),
        ))]));

        for (label, keys) in bindings {
            let keys_str = keys
                .as_ref()
                .map_or_else(|| "None".to_string(), |k| k.join(", "));
            rows.push(Row::new(vec![
                Cell::from(format!("  {label}")),
                Cell::from(keys_str),
            ]));
        }
        rows.push(Row::new(vec![Cell::from("")])); // Spacer
    }

    let table = Table::new(
        rows,
        [Constraint::Percentage(40), Constraint::Percentage(60)],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(blue)) // Changed to blue since lavender might not be in palette
            .title(" Help (Esc to close) ")
            .title_alignment(Alignment::Center),
    )
    .style(Style::default().bg(base).fg(text));

    f.render_widget(Clear, area);
    f.render_widget(table, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
