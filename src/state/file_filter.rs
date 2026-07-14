pub struct FileFilterState {
    pub is_visible: bool,
    pub pattern: String,
    pub cursor_position: usize,
    pub error: Option<String>,
}

impl FileFilterState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            pattern: String::new(),
            cursor_position: 0,
            error: None,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.pattern.clear();
        self.cursor_position = 0;
        self.error = None;
    }
}

impl crate::state::PopupState for FileFilterState {
    fn reset(&mut self) {
        self.is_visible = false;
        self.pattern.clear();
        self.cursor_position = 0;
        self.error = None;
    }

    fn set_visible(&mut self, visible: bool) {
        self.is_visible = visible;
    }
}

impl Default for FileFilterState {
    fn default() -> Self {
        Self::new()
    }
}
