use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CopyMoveAction {
    Copy,
    Move,
}

pub struct CopyMoveState {
    pub is_visible: bool,
    pub action: CopyMoveAction,
    pub source_paths: Vec<PathBuf>,
    pub destination_input: String,
    pub cursor_position: usize,
    pub input_selected: bool,
    pub error: Option<String>,
}

impl CopyMoveState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            action: CopyMoveAction::Copy,
            source_paths: Vec::new(),
            destination_input: String::new(),
            cursor_position: 0,
            input_selected: false,
            error: None,
        }
    }

    /// Opens the copy/move popup for `source_paths` and `action`, pre-filled
    /// with `destination` and the cursor at its end. Visibility is set here, so
    /// a caller cannot show the popup without populating it.
    pub fn show(
        &mut self,
        source_paths: Vec<PathBuf>,
        action: CopyMoveAction,
        destination: String,
    ) {
        self.cursor_position = destination.chars().count();
        self.source_paths = source_paths;
        self.action = action;
        self.destination_input = destination;
        self.input_selected = false;
        self.error = None;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.source_paths.clear();
        self.destination_input.clear();
        self.cursor_position = 0;
        self.input_selected = false;
        self.error = None;
    }
}

impl Default for CopyMoveState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copymoveaction_equality() {
        assert_eq!(CopyMoveAction::Copy, CopyMoveAction::Copy);
        assert_eq!(CopyMoveAction::Move, CopyMoveAction::Move);
        assert_ne!(CopyMoveAction::Copy, CopyMoveAction::Move);
    }

    #[test]
    fn test_new_and_default_state() {
        let s1 = CopyMoveState::new();
        let s2 = CopyMoveState::default();
        assert!(!s1.is_visible);
        assert_eq!(s1.action, CopyMoveAction::Copy);
        assert_eq!(s1.source_paths.len(), 0);
        assert_eq!(s1.destination_input, "");
        assert_eq!(s1.cursor_position, 0);
        assert!(!s1.input_selected);
        assert_eq!(s1.error, None);
        // Default state matches new
        assert_eq!(s1.is_visible, s2.is_visible);
        assert_eq!(s1.action, s2.action);
        assert_eq!(s1.source_paths, s2.source_paths);
        assert_eq!(s1.destination_input, s2.destination_input);
        assert_eq!(s1.cursor_position, s2.cursor_position);
        assert_eq!(s1.input_selected, s2.input_selected);
        assert_eq!(s1.error, s2.error);
    }

    #[test]
    fn test_reset_state() {
        let mut st = CopyMoveState {
            is_visible: true,
            action: CopyMoveAction::Move,
            source_paths: vec![PathBuf::from("/foo"), PathBuf::from("/bar")],
            destination_input: "/baz".into(),
            cursor_position: 42,
            input_selected: true,
            error: Some("err".into()),
        };
        st.reset();
        assert!(!st.is_visible);
        assert_eq!(st.source_paths.len(), 0);
        assert_eq!(st.destination_input, "");
        assert_eq!(st.cursor_position, 0);
        assert!(!st.input_selected);
        assert_eq!(st.error, None);
    }
}
