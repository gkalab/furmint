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
