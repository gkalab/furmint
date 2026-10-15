pub struct RenameTabState {
    pub is_visible: bool,
    pub new_name: String,
    pub cursor_position: usize,
}

impl RenameTabState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            new_name: String::new(),
            cursor_position: 0,
        }
    }

    /// Opens the rename-tab popup pre-filled with `current_title` and the
    /// cursor at its end. Visibility is set here, so a caller cannot show the
    /// popup without populating it.
    pub fn show(&mut self, current_title: String) {
        self.cursor_position = current_title.chars().count();
        self.new_name = current_title;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.new_name.clear();
        self.cursor_position = 0;
    }
}

impl Default for RenameTabState {
    fn default() -> Self {
        Self::new()
    }
}
