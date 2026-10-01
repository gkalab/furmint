use crate::state::bookmark::BookmarkState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

pub fn draw_bookmark_popup(f: &mut Frame, state: &mut BookmarkState, palette: &ThemePalette) {
    if !state.list.is_visible {
        return;
    }

    crate::ui::filterable_list::draw_filterable_list_popup(f, &mut state.list, palette);

    // Draw confirmation overlay if active
    if let Some(conf) = state.confirmation.as_mut() {
        let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
        crate::ui::ui_utils::draw_confirmation_popup(f, conf, palette, 60, 6, bg_color);
    }
}
