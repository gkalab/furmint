use crate::config::KeyboardConfig;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table};
use termina::event::KeyCode as CrosstermKeyCode;

pub fn draw_help_popup(
    f: &mut ratatui::Frame,
    app: &mut crate::app::AppState, // Takes app state mutable to update scroll calculation limits? No, we shouldn't update state during draw. But we need scroll_offset.
    keyboard: &KeyboardConfig,
    palette: &ThemePalette,
) {
    if !app.popups.help.is_visible {
        return;
    }

    let size = f.area();
    // Using unified centered rect utility
    let area = crate::ui::ui_utils::centered_rect_percent(40, 80, size);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    f.render_widget(Clear, area);

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(ratatui::symbols::border::EMPTY)
            .style(Style::default().bg(bg_color)),
        area,
    );

    // Inner chunks for margin
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([Constraint::Min(1)])
        .split(area);

    // Draw inner block with field background
    let inner_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::EMPTY)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .style(Style::default().bg(field_bg_color));

    f.render_widget(inner_block.clone(), chunks[0]);
    let inner_content_area = inner_block.inner(chunks[0]);

    // Layout inside inner block: Title, Content (Table)
    let layout = Layout::vertical([
        Constraint::Length(2), // Title
        Constraint::Min(1),    // Table
    ])
    .horizontal_margin(1)
    .split(inner_content_area);

    f.render_widget(
        Paragraph::new("Help (Esc to close)")
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(text_color)
                    .add_modifier(Modifier::BOLD)
                    .bg(field_bg_color),
            ),
        layout[0],
    );

    let table_area = layout[1];
    app.popups.help.table_area = Some(table_area);

    let (rows, total_rows) = build_help_rows(keyboard, border_color);
    app.popups.help.total_rows = total_rows;
    let visible_height = table_area.height as usize;

    // Adjust scroll offset so the last row sits at the bottom of the visible area
    let max_scroll = total_rows.saturating_sub(visible_height);
    if app.popups.help.scroll_offset > max_scroll {
        app.popups.help.scroll_offset = max_scroll;
    }

    let scroll_offset = app.popups.help.scroll_offset;
    let table_style = Style::default().bg(field_bg_color).fg(text_color);
    render_help_table(
        f,
        rows,
        table_area,
        visible_height,
        scroll_offset,
        table_style,
        palette,
    );
}

pub fn handle_help_popup_event(code: CrosstermKeyCode, app: &mut crate::app::AppState) -> bool {
    // visible_height unused for now as we use hardcoded page_step
    // Ideally, we store visible_height in state during draw, but for now we can assume a safe default or use a larger step.
    // Or better, we only increment/decrement by 1 for arrow keys, and use a fixed step for page up/down.
    // However, without knowing the rendered height, exact page scrolling is hard.
    // Let's use a reasonable constant for page scroll, or just 10.
    let page_step = 10;

    // Total rows is also dynamic based on config. We can calculate it again or store it.
    // Re-calculating cost is low.
    // But we don't have access to keyboard config here easily to calculate exact rows.
    // We can just guard against overflow in the draw function (which we did), passing a simplified max here?
    // Actually, simply incrementing indefinitely is fine if draw clamps it?
    // Yes, draw function clamps `app.popups.help.scroll_offset`.
    // BUT, we need to know total rows to clamp here if we want "End" key to work perfectly immediately,
    // or we rely on draw to clamp it on next frame.
    // Let's rely on draw clamping for upper bound? No, because we modify state here.
    // If we set it to usize::MAX, draw checks `if scroll > max { scroll = max }`.
    // So for "End", we can set it to usize::MAX.

    match code {
        CrosstermKeyCode::Escape => {
            app.popups.help.reset();
        }
        CrosstermKeyCode::Up => {
            app.popups.help.scroll_offset = app.popups.help.scroll_offset.saturating_sub(1);
        }
        CrosstermKeyCode::Down => {
            app.popups.help.scroll_offset = app.popups.help.scroll_offset.saturating_add(1);
        }
        CrosstermKeyCode::PageUp => {
            app.popups.help.scroll_offset = app.popups.help.scroll_offset.saturating_sub(page_step);
        }
        CrosstermKeyCode::PageDown => {
            app.popups.help.scroll_offset = app.popups.help.scroll_offset.saturating_add(page_step);
        }
        CrosstermKeyCode::Home => {
            app.popups.help.scroll_offset = 0;
        }
        CrosstermKeyCode::End => {
            app.popups.help.scroll_offset = usize::MAX; // Draw will clamp
        }
        _ => return false,
    }
    true
}

#[allow(clippy::type_complexity)]
fn build_help_categories(
    keyboard: &crate::config::KeyboardConfig,
) -> Vec<(&'static str, Vec<(&'static str, &Option<Vec<String>>)>)> {
    let base_nav = vec![
        ("Search", &keyboard.search),
        ("Enter Dir", &keyboard.enter_dir),
        ("Up Dir", &keyboard.up_dir),
        ("New Tab", &keyboard.new_tab),
        ("Next Tab", &keyboard.tab_next),
        ("Prev Tab", &keyboard.tab_prev),
        ("Swap Tabs", &keyboard.swap_tabs),
        ("Close Tab", &keyboard.tab_close),
        ("Rename Tab", &keyboard.rename_tab),
        ("Move Tab Left", &keyboard.tab_move_left),
        ("Move Tab Right", &keyboard.tab_move_right),
    ];

    #[cfg(target_os = "windows")]
    let navigation = {
        let mut v = base_nav;
        v.push(("Change Drive of Left Panel", &keyboard.change_drive_left));
        v.push(("Change Drive of Right Panel", &keyboard.change_drive_right));
        v
    };

    #[cfg(not(target_os = "windows"))]
    let navigation = base_nav;

    vec![
        ("Navigation", navigation),
        (
            "Bookmarks",
            vec![
                ("Add Bookmark", &keyboard.add_bookmark),
                ("Open Bookmarks", &keyboard.open_bookmarks),
            ],
        ),
        (
            "File Operations",
            vec![
                ("New File", &keyboard.new_file),
                ("New Dir", &keyboard.new_dir),
                ("Select All", &keyboard.select_all),
                ("Edit", &keyboard.edit_file),
                ("Copy", &keyboard.copy_to),
                ("Move", &keyboard.move_to),
                ("Rename", &keyboard.rename),
                ("Delete", &keyboard.delete),
                ("Force Delete", &keyboard.delete_force),
                ("Empty Trash", &keyboard.empty_trash),
                ("Calculate Directory Size", &keyboard.calc_dir_size),
                ("File Filter", &keyboard.file_filter),
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
                ("Toggle Console", &keyboard.toggle_console),
                ("Terminal", &keyboard.open_terminal),
                ("Quit", &keyboard.quit),
            ],
        ),
        (
            "SSH",
            vec![
                ("New Connection", &keyboard.open_ssh),
                ("Reconnect", &keyboard.reconnect_ssh),
            ],
        ),
        (
            "File Viewer",
            vec![
                ("Toggle Viewer", &keyboard.toggle_viewer),
                ("Search", &keyboard.viewer_search),
                ("Search Next", &keyboard.viewer_search_next),
                ("Search Previous", &keyboard.viewer_search_prev),
            ],
        ),
    ]
}

fn build_help_rows(keyboard: &KeyboardConfig, border_color: Color) -> (Vec<Row<'static>>, usize) {
    let categories = build_help_categories(keyboard);

    let mut rows = Vec::new();
    for (category, bindings) in categories {
        rows.push(Row::new(vec![Cell::from(Span::styled(
            category,
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(border_color),
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
        rows.push(Row::new(vec![Cell::from("")]));
    }

    let total = rows.len();
    (rows, total)
}

fn render_help_table(
    f: &mut ratatui::Frame,
    rows: Vec<Row<'static>>,
    table_area: Rect,
    visible_height: usize,
    scroll_offset: usize,
    table_style: Style,
    palette: &ThemePalette,
) {
    let total_rows = rows.len();
    let rows_to_render = rows.into_iter().skip(scroll_offset).take(visible_height);

    let table = Table::new(
        rows_to_render,
        [Constraint::Percentage(60), Constraint::Percentage(40)],
    )
    .style(table_style);

    f.render_widget(table, table_area);

    let scroll_area = Rect {
        x: table_area.x + table_area.width.saturating_sub(1),
        y: table_area.y,
        width: 1,
        height: table_area.height,
    };

    crate::ui::ui_utils::draw_scrollbar(
        f,
        scroll_area,
        total_rows,
        visible_height,
        scroll_offset,
        palette,
        true,
    );
}
