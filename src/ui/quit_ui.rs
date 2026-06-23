use crate::app::QuitConfirmationState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_quit_popup(
    f: &mut ratatui::Frame,
    state: &mut QuitConfirmationState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let message = "There are running tasks!\nAre you sure you want to quit?";

    let popup_area = crate::ui::ui_utils::centered_rect_absolute(50, 8, f.area());
    state.popup_area = popup_area;

    let confirmation_state = crate::state::ConfirmationState {
        is_visible: true,
        message: message.to_string(),
        truncate: false,
        action: crate::state::ConfirmationAction::None,
        selected_no: state.selected_no,
    };

    crate::ui::ui_utils::draw_confirmation_popup(f, &confirmation_state, palette, 50, 7, bg_color);

    // Compute button areas for the confirmation popup layout
    let content_area = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(1),
    };

    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(3),
        ])
        .split(content_area);

    state.button_areas =
        crate::ui::ui_utils::compute_button_rects(&["(N)o", "(Y)es"], inner_layout[2]);
}
