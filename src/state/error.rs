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
mod tests {
    use super::*;

    #[test]
    fn test_new_and_default() {
        let state = ErrorState::new();
        assert!(!state.is_visible);
        assert_eq!(state.task_id, 0);
        assert_eq!(state.error_path, "");
        assert_eq!(state.error_message, "");

        let default_state = ErrorState::default();
        assert_eq!(state.is_visible, default_state.is_visible);
        assert_eq!(state.task_id, default_state.task_id);
        assert_eq!(state.error_path, default_state.error_path);
        assert_eq!(state.error_message, default_state.error_message);
    }

    #[test]
    fn test_reset() {
        let mut state = ErrorState {
            is_visible: true,
            task_id: 123,
            error_path: String::from("some/path"),
            error_message: String::from("error occurred"),
            focused_button: 0,
        };
        state.reset();
        assert!(!state.is_visible);
        assert_eq!(state.task_id, 0);
        assert_eq!(state.error_path, "");
        assert_eq!(state.error_message, "");
    }
}
