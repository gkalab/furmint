use std::path::PathBuf;

use crate::tasks::ConflictType;

pub struct ConflictState {
    pub is_visible: bool,
    pub task_id: usize,
    pub conflict_path: PathBuf,
    pub conflict_type: ConflictType,
    pub focused_button: usize,
}

impl ConflictState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            task_id: 0,
            conflict_path: PathBuf::new(),
            conflict_type: ConflictType::FileExists,
            focused_button: 0,
        }
    }

    /// Opens the conflict popup for `task_id`, showing the conflicting `path`
    /// and how to resolve it. Visibility is set here, so a caller cannot show
    /// the popup without populating it.
    pub fn show(&mut self, task_id: usize, path: PathBuf, conflict_type: ConflictType) {
        self.task_id = task_id;
        self.conflict_path = path;
        self.conflict_type = conflict_type;
        self.focused_button = 0;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.task_id = 0;
        self.conflict_path = PathBuf::new();
        self.focused_button = 0;
    }
}

impl Default for ConflictState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
