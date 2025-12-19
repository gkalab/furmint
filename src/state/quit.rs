pub struct QuitConfirmationState {
    pub is_visible: bool,
}

impl QuitConfirmationState {
    pub fn new() -> Self {
        Self { is_visible: false }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
    }
}

impl Default for QuitConfirmationState {
    fn default() -> Self {
        Self::new()
    }
}
