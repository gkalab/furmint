use std::path::PathBuf;

#[allow(clippy::struct_excessive_bools)] // independent flags: visibility, mode and the trash hint
pub struct DeleteState {
    pub is_visible: bool,
    pub selected_paths: Vec<PathBuf>,
    /// The delete is permanent (no move-to-trash).
    pub is_permanent: bool,
    /// The filesystem offers no trash, so a permanent delete is the only
    /// option. Kept separate from `is_permanent` so the prompt can say *why*
    /// the safe choice is unavailable instead of silently promoting the
    /// destructive key.
    pub no_trash: bool,
    pub selected_no: bool,
    pub error: Option<String>,
}

impl DeleteState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            selected_paths: Vec::new(),
            is_permanent: false,
            no_trash: false,
            selected_no: true,
            error: None,
        }
    }

    /// Opens the delete popup for `selected_paths`, defaulting the choice to
    /// "no". Visibility is set here, so a caller cannot show the popup without
    /// populating it.
    ///
    /// `no_trash` says the filesystem has no trash; the delete is then
    /// permanent, and the prompt names that rather than presenting a "Trash"
    /// question it would silently answer with a permanent delete.
    pub fn show(&mut self, selected_paths: Vec<PathBuf>, permanent: bool, no_trash: bool) {
        self.selected_paths = selected_paths;
        self.is_permanent = permanent || no_trash;
        self.no_trash = no_trash;
        self.selected_no = true;
        self.error = None;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.selected_paths.clear();
        self.is_permanent = false;
        self.no_trash = false;
        self.selected_no = true;
        self.error = None;
    }
}

impl Default for DeleteState {
    fn default() -> Self {
        Self::new()
    }
}
