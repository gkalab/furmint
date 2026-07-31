pub struct HelpState {
    pub is_visible: bool,
    pub scroll_offset: usize,
    pub table_area: Option<ratatui::layout::Rect>,
    pub total_rows: usize,
}

impl HelpState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            scroll_offset: 0,
            table_area: None,
            total_rows: 0,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.scroll_offset = 0;
        self.table_area = None;
        self.total_rows = 0;
    }
}

impl Default for HelpState {
    fn default() -> Self {
        Self::new()
    }
}
