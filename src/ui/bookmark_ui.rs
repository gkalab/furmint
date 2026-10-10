use crate::state::bookmark::BookmarkState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_bookmark_popup(
    f: &mut Frame,
    state: &mut BookmarkState,
    geometry: &crate::popup_layout::BookmarkGeometry,
    palette: &ThemePalette,
) {
    if !state.list.is_visible {
        return;
    }

    crate::ui::filterable_list::draw_filterable_list_popup(
        f,
        &mut state.list,
        geometry.list_area,
        palette,
    );

    // Draw confirmation overlay if active
    if let Some(conf) = state.confirmation.as_ref() {
        let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
        crate::ui::ui_utils::draw_confirmation_popup(
            f,
            conf,
            &geometry.confirmation_buttons,
            palette,
            60,
            6,
            bg_color,
        );
    }
}
