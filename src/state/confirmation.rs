pub struct ConfirmationState {
    pub is_visible: bool,
    pub message: String,
    pub truncate: bool,
    pub action: ConfirmationAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationAction {
    DeleteSshHistory(usize),
    None,
}

impl ConfirmationState {
    pub fn new(message: String, truncate: bool, action: ConfirmationAction) -> Self {
        Self {
            is_visible: true,
            message,
            truncate,
            action,
        }
    }
}
