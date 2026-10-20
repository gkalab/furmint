use super::layout::{centered_rect_absolute, field_border_set};
use crate::theme::ThemePalette;
use crate::ui::text_edit::{TextEditState, TextEditWidget, render_text_edit};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Options for an input popup: a title, the live editor state, an optional error
/// line below the field, placeholder text and the popup width.
pub struct InputPopupOptions<'a> {
    pub title: Option<&'a str>,
    pub input: &'a TextEditState,
    pub error: Option<&'a str>,
    pub placeholder: &'a str,
    pub width: u16,
}

#[allow(clippy::struct_field_names)] // the `_color` suffix is the point
pub struct InputStyleContext {
    pub text_color: Color,
    pub placeholder_color: Color,
    pub field_bg_color: Color,
    pub selection_style: Style,
}

impl InputStyleContext {
    #[must_use]
    pub fn new(
        text_color: Color,
        placeholder_color: Color,
        field_bg_color: Color,
        selection_style: Style,
    ) -> Self {
        Self {
            text_color,
            placeholder_color,
            field_bg_color,
            selection_style,
        }
    }
}

/// Style for a selected run of text in an input field, matching the list
/// selection look (shared with the SSH popup).
fn selection_style(palette: &ThemePalette, text_color: Color) -> Style {
    let background = Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b);
    let foreground = if palette.is_dark {
        text_color
    } else {
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    };
    Style::default().fg(foreground).bg(background)
}

/// The clickable text row of an input popup, in screen coordinates.
///
/// Objectively reproduces the layout of [`draw_input_popup`] for a popup of
/// `popup_width` (with or without a title), so rendering and mouse hit-testing
/// can never disagree about where the field is.
#[must_use]
pub fn input_popup_text_area(popup_width: u16, has_title: bool, screen: Rect) -> Rect {
    let popup_height = if has_title { 7 } else { 5 };
    let popup_area = centered_rect_absolute(popup_width, popup_height, screen);

    let chunk = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([Constraint::Length(if has_title { 5 } else { 3 })])
        .split(popup_area)[0];

    let input_row = if has_title { 3 } else { 1 };
    Rect {
        x: chunk.x.saturating_add(2),
        y: chunk.y.saturating_add(input_row),
        width: chunk.width.saturating_sub(4),
        height: 1,
    }
}

pub fn draw_input_popup(
    f: &mut ratatui::Frame,
    options: &InputPopupOptions,
    palette: &crate::theme::ThemePalette,
) {
    let has_title = options.title.is_some();
    let popup_width = options.width;
    let popup_height = if has_title { 7 } else { 5 };
    let popup_area = centered_rect_absolute(popup_width, popup_height, f.area());

    // Clear the popup area
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let title_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let error_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let placeholder_color = Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);

    // Draw outer block (shadow/background)
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(ratatui::symbols::border::EMPTY)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    // Inner chunks for vertical spacing
    let constraints = if has_title {
        vec![Constraint::Length(5)]
    } else {
        vec![Constraint::Length(3)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints(constraints)
        .split(popup_area);

    // Draw input block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .border_set(field_border_set())
        .style(Style::default().bg(field_bg_color));

    f.render_widget(&block, chunks[0]);

    // Layout inside padding for title, input, error
    let text_area = Rect {
        x: chunks[0].x + 2,
        y: chunks[0].y,
        width: chunks[0].width.saturating_sub(2),
        height: chunks[0].height,
    };

    let layout_constraints = if has_title {
        vec![
            Constraint::Length(1), // Spacing (top border)
            Constraint::Length(1), // Title
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Input
            Constraint::Length(1), // Error
        ]
    } else {
        vec![
            Constraint::Length(1), // Spacing (top border)
            Constraint::Length(1), // Input
            Constraint::Length(1), // Error
        ]
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(layout_constraints)
        .split(text_area);

    if let Some(t) = options.title {
        let title_paragraph = Paragraph::new(t)
            .style(Style::default().fg(title_color).bg(field_bg_color))
            .alignment(Alignment::Left);
        f.render_widget(title_paragraph, layout[1]);
    }

    let input_area = input_popup_text_area(popup_width, has_title, f.area());
    let style = InputStyleContext::new(
        text_color,
        placeholder_color,
        field_bg_color,
        selection_style(palette, text_color),
    );
    draw_input_text_and_cursor(f, options.input, options.placeholder, input_area, &style);

    // Error
    let error_index = if has_title { 4 } else { 2 };
    if let Some(error) = options.error {
        let error_paragraph = Paragraph::new(error)
            .style(Style::default().fg(error_color).bg(field_bg_color))
            .alignment(Alignment::Right);
        f.render_widget(error_paragraph, layout[error_index]);
    }
}

fn draw_input_text_and_cursor(
    f: &mut ratatui::Frame,
    input: &TextEditState,
    placeholder: &str,
    text_area: Rect,
    style: &InputStyleContext,
) {
    let widget = TextEditWidget::new(input, placeholder, true)
        .text_color(style.text_color)
        .placeholder_color(style.placeholder_color)
        .selection_style(style.selection_style)
        .bg(style.field_bg_color);
    render_text_edit(f, widget, text_area);
}
