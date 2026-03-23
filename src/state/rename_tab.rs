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

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.new_name.clear();
        self.cursor_position = 0;
    }
}

impl crate::state::PopupState for RenameTabState {
    fn reset(&mut self) {
        self.reset();
    }

    fn set_visible(&mut self, visible: bool) {
        self.is_visible = visible;
    }
}

impl Default for RenameTabState {
    fn default() -> Self {
        Self::new()
    }
}
