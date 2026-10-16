use crate::ui::text_edit::TextEditState;

pub struct RenameTabState {
    pub is_visible: bool,
    pub new_name: TextEditState,
}

impl RenameTabState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            new_name: TextEditState::new(),
        }
    }

    /// Opens the rename-tab popup pre-filled with `current_title` and the
    /// cursor at its end. Visibility is set here, so a caller cannot show the
    /// popup without populating it.
    pub fn show(&mut self, current_title: String) {
        self.new_name.set_text(current_title);
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.new_name.clear();
    }
}

impl Default for RenameTabState {
    fn default() -> Self {
        Self::new()
    }
}
