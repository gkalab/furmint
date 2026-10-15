use crate::state::confirmation::ConfirmationState;
use crate::ui::filterable_list::FilterableListState;

#[derive(Default)]
pub struct BookmarkState {
    pub list: FilterableListState,
    pub confirmation: Option<ConfirmationState>,
}

impl BookmarkState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            list: FilterableListState::new(),
            confirmation: None,
        }
    }

    /// Opens the bookmark list with an empty filter, ready to be filled by
    /// [`crate::handlers::popup_bookmark::refresh_bookmark_list`]. Visibility is
    /// set here, so a caller cannot show the list without resetting it.
    pub fn show(&mut self) {
        self.list.input.clear();
        self.list.cursor_position = 0;
        self.list.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.list.reset();
        self.confirmation = None;
    }
}
