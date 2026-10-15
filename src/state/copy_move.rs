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
mod tests;
