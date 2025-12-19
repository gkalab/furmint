pub struct ErrorState {
    pub is_visible: bool,
    pub task_id: usize,
    pub error_path: String,
    pub error_message: String,
}

impl ErrorState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            task_id: 0,
            error_path: String::new(),
            error_message: String::new(),
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.task_id = 0;
        self.error_path.clear();
        self.error_message.clear();
    }
}

impl Default for ErrorState {
    fn default() -> Self {
        Self::new()
    }
}
