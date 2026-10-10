pub struct ConfirmationState {
    pub is_visible: bool,
    pub message: String,
    pub truncate: bool,
    pub action: ConfirmationAction,
    pub selected_no: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationAction {
    DeleteSshHistory(usize),
    DeleteBookmark(usize),
    None,
}

impl ConfirmationState {
    #[must_use]
    pub fn new(message: String, truncate: bool, action: ConfirmationAction) -> Self {
        Self {
            is_visible: true,
            message,
            truncate,
            action,
            selected_no: true,
        }
    }
}
