use crate::app::QuitConfirmationState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw_quit_popup(
    f: &mut ratatui::Frame,
    state: &QuitConfirmationState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 50;
    let popup_height = 8;
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
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" Warning ")
        .title_alignment(Alignment::Center)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    // Draw block/borders first
    f.render_widget(&block, popup_area);
    let mut inner_area = block.inner(popup_area);
    // Add margin so content/buttons never touch borders
    inner_area.x += 1;
    inner_area.width = inner_area.width.saturating_sub(2);
	// Optionally margin on y as well for more padding

    let layout = Layout::vertical([
        Constraint::Min(2), // Message
        Constraint::Length(1), // Button row
    ]).split(inner_area);

    let message = "There are running tasks!\nAre you sure you want to quit?";
    let p = Paragraph::new(message)
        .alignment(Alignment::Center)
        .style(Style::default().fg(text_color));
    f.render_widget(p, layout[0]);

    crate::ui_utils::draw_button_row(
        f,
        &["[Y]es", "[N]o"],
        layout[1],
        text_color,
    );
}
