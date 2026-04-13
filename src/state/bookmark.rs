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

    pub fn reset(&mut self) {
        self.list.reset();
        self.confirmation = None;
    }
}
