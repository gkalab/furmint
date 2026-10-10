pub struct QuitConfirmationState {
    pub is_visible: bool,
    pub selected_no: bool,
}

impl QuitConfirmationState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            selected_no: true,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.selected_no = true;
    }
}

impl Default for QuitConfirmationState {
    fn default() -> Self {
        Self::new()
    }
}
