pub struct HelpState {
    pub is_visible: bool,
}

impl HelpState {
    pub fn new() -> Self {
        Self { is_visible: false }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
    }
}

impl Default for HelpState {
    fn default() -> Self {
        Self::new()
    }
}
