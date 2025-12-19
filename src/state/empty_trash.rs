pub struct EmptyTrashState {
    pub is_visible: bool,
}

impl EmptyTrashState {
    pub fn new() -> Self {
        Self { is_visible: false }
    }
}

impl Default for EmptyTrashState {
    fn default() -> Self {
        Self::new()
    }
}
