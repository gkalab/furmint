use crate::state::ConflictState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Number of buttons on the conflict popup's first and second rows.
const CONFLICT_ROW1_BUTTONS: usize = 3;
const CONFLICT_ROW2_BUTTONS: usize = 2;

pub fn draw_conflict_popup(
    f: &mut ratatui::Frame,
    state: &ConflictState,
    geometry: &crate::popup_layout::ButtonGeometry,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 60;
    let popup_height = 12;
    let popup_area = crate::ui::ui_utils::centered_rect_absolute(popup_width, popup_height, area);

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b); // Red for warning
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let message_border = crate::ui::ui_utils::message_border_set();

    f.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(border_color))
            .border_set(message_border)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let field_bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);

    let content_area = Rect {
        x: popup_area.x + 3,
        y: popup_area.y + 2,
        width: popup_area.width.saturating_sub(6),
        height: popup_area.height.saturating_sub(2),
    };

    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // Message + Path
            Constraint::Length(1), // Spacing
            Constraint::Length(3), // Row 1 buttons
            Constraint::Length(3), // Row 2 buttons
        ])
        .split(content_area);

    let content = format!(
        "File already exists:\n\n{}\n",
        state.conflict_path.display()
    );

    // We render the explanation text
    let p_text = Paragraph::new(content)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(text_color).bg(field_bg_color));
    f.render_widget(p_text, inner_layout[0]);

    let row1_focus = (state.focused_button < 3).then_some(state.focused_button);
    let row2_focus = (state.focused_button >= 3).then(|| state.focused_button - 3);

    // Row 1 holds the first three buttons, row 2 the remaining two.
    let split_at = geometry
        .button_areas
        .len()
        .saturating_sub(CONFLICT_ROW2_BUTTONS)
        .min(CONFLICT_ROW1_BUTTONS);
    let (row1, row2) = geometry.button_areas.split_at(split_at);

    crate::ui::ui_utils::draw_button_row_at(
        f,
        &["[C]ancel", "[S]kip", "[O]verwrite"],
        row1,
        palette,
        field_bg_color,
        row1_focus,
    );

    crate::ui::ui_utils::draw_button_row_at(
        f,
        &["Ski[p] All", "Overwrite [A]ll"],
        row2,
        palette,
        field_bg_color,
        row2_focus,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    #[test]
    fn test_draw_popup_conflict_visible_does_not_panic() {
        // Setup test backend and Frame
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = crate::state::ConflictState {
                    is_visible: true,
                    task_id: 42,
                    conflict_path: PathBuf::from("/tmp/existing.txt"),
                    conflict_type: crate::tasks::ConflictType::FileExists,
                    focused_button: 0,
                };
                let palette = crate::theme::default_theme();
                // Should not panic
                draw_conflict_popup(
                    f,
                    &state,
                    &crate::popup_layout::conflict_geometry(Rect::new(0, 0, 80, 24), true),
                    &palette,
                );
            })
            .unwrap();
    }

    #[test]
    fn test_draw_popup_conflict_invisible_does_nothing() {
        // Setup test backend and Frame
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = crate::state::ConflictState::default(); // is_visible = false
                let palette = crate::theme::default_theme();
                // Should just return, nothing rendered
                draw_conflict_popup(
                    f,
                    &state,
                    &crate::popup_layout::conflict_geometry(Rect::new(0, 0, 80, 24), true),
                    &palette,
                );
            })
            .unwrap();
    }
}
