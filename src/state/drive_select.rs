use crate::app_state::tabs::PanelSide;

/// A selectable drive: the path a pane navigates to, plus optional detail
/// text (e.g. size info) shown right-aligned next to the path.
#[derive(Debug, Clone)]
pub struct Drive {
    /// The path the pane navigates to when this drive is selected.
    pub path: String,
    /// Detail text shown in the right-hand column; empty if none.
    pub detail: String,
    /// Compact form of the detail (e.g. "930.4G/146.2G/84%") used when the
    /// full detail does not fit the column; empty if none.
    pub detail_short: String,
}

pub struct DriveSelectState {
    pub is_visible: bool,
    pub drives: Vec<Drive>,
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
    pub fn show(&mut self, drives: Vec<Drive>, side: PanelSide) {
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
