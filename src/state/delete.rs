use std::path::PathBuf;

pub struct DeleteState {
    pub is_visible: bool,
    pub selected_paths: Vec<PathBuf>,
    pub is_permanent: bool,
    pub selected_no: bool,
    pub error: Option<String>,
    pub popup_area: ratatui::layout::Rect,
    pub button_areas: Vec<ratatui::layout::Rect>,
}

impl DeleteState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            selected_paths: Vec::new(),
            is_permanent: false,
            selected_no: true,
            error: None,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.selected_paths.clear();
        self.is_permanent = false;
        self.selected_no = true;
        self.error = None;
        self.popup_area = ratatui::layout::Rect::default();
        self.button_areas.clear();
    }
}

impl crate::state::PopupState for DeleteState {
    fn reset(&mut self) {
        self.reset();
    }

    fn set_visible(&mut self, visible: bool) {
        self.is_visible = visible;
    }
}

impl Default for DeleteState {
    fn default() -> Self {
        Self::new()
    }
}
