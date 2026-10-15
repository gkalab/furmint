use std::path::PathBuf;

pub struct CreateFileState {
    pub is_visible: bool,
    pub input_value: String,
    pub cursor_position: usize,
    pub error: Option<String>,
    pub parent_dir: PathBuf,
}

impl CreateFileState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            input_value: String::new(),
            cursor_position: 0,
            error: None,
            parent_dir: PathBuf::new(),
        }
    }

    /// Opens the create-file popup in `parent_dir` with an empty input.
    /// Visibility is set here, so a caller cannot show the popup without
    /// populating it.
    pub fn show(&mut self, parent_dir: PathBuf) {
        self.input_value.clear();
        self.cursor_position = 0;
        self.error = None;
        self.parent_dir = parent_dir;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.input_value.clear();
        self.cursor_position = 0;
        self.error = None;
        self.parent_dir = PathBuf::new();
    }
}

impl Default for CreateFileState {
    fn default() -> Self {
        Self::new()
    }
}
