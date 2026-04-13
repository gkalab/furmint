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

    // Compatibility methods delegating to the inner list
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.list.is_visible
    }
    pub fn set_visible(&mut self, visible: bool) {
        self.list.is_visible = visible;
    }
    #[must_use]
    pub fn input(&self) -> &str {
        &self.list.input
    }
    #[must_use]
    pub fn filtered_dirs(&self) -> &[PathBuf] {
        &self.list.items
    }
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.list.selected_index
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
        self.list.get_selected_item()
    }

    pub fn update_scroll(&mut self, visible_rows: usize) {
        self.list.update_scroll(visible_rows);
    }

    pub fn move_cursor_left(&mut self) {
        self.list.move_cursor_left();
    }
    pub fn move_cursor_right(&mut self) {
        self.list.move_cursor_right();
    }
    pub fn move_cursor_home(&mut self) {
        self.list.move_cursor_home();
    }
    pub fn move_cursor_end(&mut self) {
        self.list.move_cursor_end();
    }
}

pub fn draw_fuzzy_search_popup(
    f: &mut Frame,
    state: &mut FuzzySearchState,
    palette: &ThemePalette,
) {
    crate::ui::filterable_list::draw_filterable_list_popup(f, &mut state.list, palette);
}
