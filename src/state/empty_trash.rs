pub struct EmptyTrashState {
    pub is_visible: bool,
    pub selected_no: bool,
}

impl EmptyTrashState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            selected_no: true,
        }
    }

    /// Opens the confirmation popup defaulting to "no". The shared
    /// [`crate::handlers::popup_utils::get_choice_with_selection`] helper resets
    /// `selected_no` on confirm/cancel, so this only fires on (re)open.
    pub fn show(&mut self) {
        self.selected_no = true;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.selected_no = true;
    }
}

impl Default for EmptyTrashState {
    fn default() -> Self {
        Self::new()
    }
}
