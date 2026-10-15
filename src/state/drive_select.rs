use crate::app_state::tabs::PanelSide;

pub struct DriveSelectState {
    pub is_visible: bool,
    pub drives: Vec<String>,
    pub selected_index: usize,
    pub side: PanelSide,
}

impl DriveSelectState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            drives: Vec::new(),
            selected_index: 0,
            side: PanelSide::Left,
        }
    }

    /// Opens the drive picker for `side` with the cursor on the first drive.
    /// Visibility is set here, so a caller cannot show the popup without
    /// populating it.
    pub fn show(&mut self, drives: Vec<String>, side: PanelSide) {
        self.drives = drives;
        self.side = side;
        self.selected_index = 0;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.drives.clear();
        self.selected_index = 0;
    }
}

impl Default for DriveSelectState {
    fn default() -> Self {
        Self::new()
    }
}
