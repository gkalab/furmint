use crate::theme::ThemePalette;
use crate::ui::filterable_list::FilterableListState;
use ratatui::prelude::*;
use std::path::PathBuf;

#[derive(Default)]
pub struct FuzzySearchState {
    pub list: FilterableListState,
}

impl FuzzySearchState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            list: FilterableListState::new(),
        }
    }

    pub fn reset(&mut self) {
        self.list.reset();
    }

    pub fn move_selection_up(&mut self) {
        self.list.move_selection_up();
    }
    pub fn move_selection_down(&mut self) {
        self.list.move_selection_down();
    }
    pub fn move_selection_page_up(&mut self, page_size: usize) {
        self.list.move_selection_page_up(page_size);
    }
    pub fn move_selection_page_down(&mut self, page_size: usize) {
        self.list.move_selection_page_down(page_size);
    }

    #[must_use]
    pub fn get_selected_dir(&self) -> Option<PathBuf> {
        self.list.get_selected_item().map(|item| item.path)
    }
}

pub fn draw_fuzzy_search_popup(
    f: &mut Frame,
    state: &mut FuzzySearchState,
    list_area: Option<Rect>,
    palette: &ThemePalette,
) {
    crate::ui::filterable_list::draw_filterable_list_popup(f, &mut state.list, list_area, palette);
}
