pub struct EmptyTrashState {
    pub is_visible: bool,
}

impl EmptyTrashState {
    #[must_use]
    pub fn new() -> Self {
        Self { is_visible: false }
    }
}

impl Default for EmptyTrashState {
    fn default() -> Self {
        Self::new()
    }
}
