pub struct CreateDirectoryState {
    pub is_visible: bool,
    pub new_name: String,
    pub cursor_position: usize,
    pub error: Option<String>,
}

impl CreateDirectoryState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            new_name: String::new(),
            cursor_position: 0,
            error: None,
        }
    }

    /// Opens the create-directory popup with an empty input. Visibility is set
    /// here, so a caller cannot show the popup without populating it.
    pub fn show(&mut self) {
        self.new_name.clear();
        self.cursor_position = 0;
        self.error = None;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.new_name.clear();
        self.cursor_position = 0;
        self.error = None;
    }
}

impl Default for CreateDirectoryState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
