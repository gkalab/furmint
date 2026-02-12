use std::path::PathBuf;

pub struct DeleteState {
    pub is_visible: bool,
    pub selected_paths: Vec<PathBuf>,
    pub is_permanent: bool,
    pub error: Option<String>,
}

impl DeleteState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            selected_paths: Vec::new(),
            is_permanent: false,
            error: None,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.selected_paths.clear();
        self.is_permanent = false;
        self.error = None;
    }
}

impl Default for DeleteState {
    fn default() -> Self {
        Self::new()
    }
}
