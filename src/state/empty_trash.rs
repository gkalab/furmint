pub struct EmptyTrashState {
    pub is_visible: bool,
    pub selected_no: bool,
}

impl EmptyTrashState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            selected_no: true,
        }
    }
}

impl Default for EmptyTrashState {
    fn default() -> Self {
        Self::new()
    }
}
