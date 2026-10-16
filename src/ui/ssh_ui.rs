use crate::app::AppState;
use crate::state::ssh::SshField;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

pub fn draw_ssh_connection_popup(f: &mut Frame, app: &mut AppState, palette: &ThemePalette) {
    let area = f.area();
    let popup_area = crate::ui::ui_utils::centered_rect_percent(70, 80, area);

    f.render_widget(Clear, popup_area);

    let active_field = app.popups.ssh_connection.active_field;
    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);

    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_set(symbols::border::EMPTY)
            .border_style(Style::default().fg(Color::Rgb(
                palette.border.r,
                palette.border.g,
                palette.border.b,
            )))
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(6),
        ])
        .split(popup_area);

    render_input_field(
        f,
        chunks[0],
        "Connection String (user@host[:/path])",
        &app.popups.ssh_connection.connection_string,
        active_field == SshField::ConnectionString,
        palette,
    );

    render_input_field(
        f,
        chunks[2],
        "Name",
        &app.popups.ssh_connection.name,
        active_field == SshField::Name,
        palette,
    );

    render_input_field(
        f,
        chunks[4],
        "Port",
        &app.popups.ssh_connection.port,
        active_field == SshField::Port,
        palette,
    );

    draw_history_list(f, chunks[6], app, palette, active_field);
}

fn render_input_field(
    f: &mut Frame,
    chunk: Rect,
    title: &str,
    state: &crate::ui::text_edit::TextEditState,
    is_active: bool,
    palette: &ThemePalette,
) {
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let placeholder_color = Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);
    let custom_border = crate::ui::ui_utils::field_border_set();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .border_set(if is_active {
            custom_border
        } else {
            symbols::border::EMPTY
        })
        .style(Style::default().bg(field_bg_color));

    f.render_widget(&block, chunk);

    let inner_area = block.inner(chunk);
    let text_area = Rect {
        x: inner_area.x + 1,
        y: inner_area.y,
        width: inner_area.width.saturating_sub(2),
        height: inner_area.height,
    };

    let widget = crate::ui::text_edit::TextEditWidget::new(state, title, is_active)
        .text_color(text_color)
        .placeholder_color(placeholder_color)
        .selection_style(selection_style(palette, text_color))
        .bg(field_bg_color);
    crate::ui::text_edit::render_text_edit(f, widget, text_area);
}

/// Style for a selected span, matching list selection.
fn selection_style(palette: &ThemePalette, text_color: Color) -> Style {
    let background = Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b);
    let foreground = if palette.is_dark {
        text_color
    } else {
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    };
    Style::default().fg(foreground).bg(background)
}

fn draw_history_list(
    f: &mut Frame,
    chunk: Rect,
    app: &mut AppState,
    palette: &ThemePalette,
    active_field: SshField,
) {
    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let block_empty = symbols::border::EMPTY;

    let history_count = app.ssh_history.connections.len();

    let history_items: Vec<ListItem> = app
        .ssh_history
        .connections
        .iter()
        .enumerate()
        .map(|(i, conn)| {
            let style = if app.popups.ssh_connection.selected_history_idx == Some(i)
                && active_field == SshField::History
            {
                let fg = if palette.is_dark {
                    text_color
                } else {
                    Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
                };
                Style::default()
                    .bg(Color::Rgb(
                        palette.surface2.r,
                        palette.surface2.g,
                        palette.surface2.b,
                    ))
                    .fg(fg)
            } else {
                Style::default().fg(text_color)
            };
            ListItem::new(conn.display_string()).style(style)
        })
        .collect();

    let history_block = Block::default()
        .borders(Borders::ALL)
        .title("Connection History")
        .border_style(Style::default().fg(border_color).bg(bg_color))
        .border_set(block_empty)
        .style(Style::default().bg(bg_color));

    let list = List::new(history_items).block(history_block);

    let mut list_state =
        ListState::default().with_selected(app.popups.ssh_connection.selected_history_idx);
    f.render_stateful_widget(list, chunk, &mut list_state);
    app.popups.ssh_connection.history_list_offset = list_state.offset();

    let scroll_area = chunk.inner(Margin {
        vertical: 1,
        horizontal: 0,
    });
    let visible_rows = scroll_area.height as usize;
    crate::ui::ui_utils::draw_scrollbar(
        f,
        scroll_area,
        history_count,
        visible_rows,
        app.popups.ssh_connection.selected_history_idx.unwrap_or(0),
        palette,
        false,
    );

    if let Some(error) = &app.popups.ssh_connection.error {
        f.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(Color::Rgb(
                palette.red.r,
                palette.red.g,
                palette.red.b,
            ))),
            chunk,
        );
    }
    if let Some(confirmation) = app.popups.ssh_connection.confirmation.as_ref() {
        let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
        crate::ui::ui_utils::draw_confirmation_popup(
            f,
            confirmation,
            &app.layout.popups.ssh_connection.confirmation_buttons,
            palette,
            66,
            6,
            bg_color,
        );
    }
}

/// Geometry of the password prompt: the popup rectangle, the bordered input
/// chunk and the inner text area. Shared by rendering and mouse hit-testing.
#[must_use]
pub fn ssh_password_geometry(screen: Rect) -> (Rect, Rect, Rect) {
    let popup_width = 60;
    let popup_height = 5;
    let popup_area = Rect {
        x: (screen.width.saturating_sub(popup_width)) / 2,
        y: (screen.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    let chunk = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([Constraint::Length(3)])
        .split(popup_area)[0];

    let text_area = Rect {
        x: chunk.x.saturating_add(2),
        y: chunk.y.saturating_add(1),
        width: chunk.width.saturating_sub(4),
        height: 1,
    };

    (popup_area, chunk, text_area)
}

pub fn draw_ssh_password_popup(f: &mut Frame, app: &AppState, palette: &ThemePalette) {
    let (popup_area, chunk, text_area) = ssh_password_geometry(f.area());

    f.render_widget(Clear, popup_area);

    let title = format!(
        "Password for {}@{}",
        app.popups.ssh_password.user, app.popups.ssh_password.host
    );

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let placeholder_color = Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);

    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(symbols::border::EMPTY)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let custom_border = crate::ui::ui_utils::field_border_set();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .border_set(custom_border)
        .style(Style::default().bg(field_bg_color));

    f.render_widget(&block, chunk);

    let widget = app
        .popups
        .ssh_password
        .password
        .widget(&title, true)
        .text_color(text_color)
        .placeholder_color(placeholder_color)
        .selection_style(selection_style(palette, text_color))
        .bg(field_bg_color);
    crate::ui::text_edit::render_text_edit(f, widget, text_area);
}
