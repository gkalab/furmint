use std::path::PathBuf;

pub struct DeleteState {
    pub is_visible: bool,
    pub selected_paths: Vec<PathBuf>,
    pub is_permanent: bool,
    pub selected_no: bool,
    pub error: Option<String>,
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
        }
    }

    /// Opens the delete popup for `selected_paths`, defaulting the choice to
    /// "no". Visibility is set here, so a caller cannot show the popup without
    /// populating it.
    pub fn show(&mut self, selected_paths: Vec<PathBuf>, permanent: bool) {
        self.selected_paths = selected_paths;
        self.is_permanent = permanent;
        self.selected_no = true;
        self.error = None;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.selected_paths.clear();
        self.is_permanent = false;
        self.selected_no = true;
        self.error = None;
    }
}

impl Default for DeleteState {
    fn default() -> Self {
        Self::new()
    }
}
