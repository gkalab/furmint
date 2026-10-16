use crate::ui::text_edit::TextEditState;
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
    pub destination_input: TextEditState,
    pub error: Option<String>,
}

impl CopyMoveState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            action: CopyMoveAction::Copy,
            source_paths: Vec::new(),
            destination_input: TextEditState::new(),
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
        self.source_paths = source_paths;
        self.action = action;
        self.destination_input.set_text(destination);
        self.error = None;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.source_paths.clear();
        self.destination_input.clear();
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
