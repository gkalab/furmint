pub struct ErrorState {
    pub is_visible: bool,
    pub task_id: usize,
    pub error_path: String,
    pub error_message: String,
    pub focused_button: usize,
}

impl ErrorState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            task_id: 0,
            error_path: String::new(),
            error_message: String::new(),
            focused_button: 0,
        }
    }

    /// Opens the error popup for `task_id`, reporting `message` for `path`.
    /// Visibility is set here, so a caller cannot show the popup without
    /// populating it.
    pub fn show(&mut self, task_id: usize, path: String, message: String) {
        self.task_id = task_id;
        self.error_path = path;
        self.error_message = message;
        self.focused_button = 0;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.task_id = 0;
        self.error_path.clear();
        self.error_message.clear();
        self.focused_button = 0;
    }
}

impl Default for ErrorState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
