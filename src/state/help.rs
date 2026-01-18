pub struct HelpState {
    pub is_visible: bool,
    pub scroll_offset: usize,
}

impl HelpState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            scroll_offset: 0,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.scroll_offset = 0;
    }
}

impl Default for HelpState {
    fn default() -> Self {
        Self::new()
    }
}
