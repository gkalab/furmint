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
