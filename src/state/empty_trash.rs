pub struct EmptyTrashState {
    pub is_visible: bool,
    pub selected_no: bool,
    pub popup_area: ratatui::layout::Rect,
    pub button_areas: Vec<ratatui::layout::Rect>,
}

impl EmptyTrashState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            selected_no: true,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.selected_no = true;
        self.popup_area = ratatui::layout::Rect::default();
        self.button_areas.clear();
    }
}

impl Default for EmptyTrashState {
    fn default() -> Self {
        Self::new()
    }
}
