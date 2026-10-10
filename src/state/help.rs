pub struct HelpState {
    pub is_visible: bool,
    pub scroll_offset: usize,
    pub total_rows: usize,
}

impl HelpState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            scroll_offset: 0,
            total_rows: 0,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.scroll_offset = 0;
        self.total_rows = 0;
    }
}

impl Default for HelpState {
    fn default() -> Self {
        Self::new()
    }
}
