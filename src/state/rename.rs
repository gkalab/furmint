use crate::ui::text_edit::TextEditState;
use std::path::PathBuf;

pub struct RenameState {
    pub is_visible: bool,
    pub new_name: TextEditState,
    pub original_name: String,
    pub parent_dir: PathBuf,
    pub show_overwrite_confirm: bool,
    pub is_dir: bool,
    pub error: Option<String>,
    pub focused_button: usize,
}

impl RenameState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            new_name: TextEditState::new(),
            original_name: String::new(),
            parent_dir: PathBuf::new(),
            show_overwrite_confirm: false,
            is_dir: false,
            error: None,
            focused_button: 0,
        }
    }

    /// Opens the rename popup for `original_name` in `parent_dir`, with the
    /// cursor placed before the extension (or at the end when there is none).
    /// Visibility is set here, so a caller cannot show the popup without
    /// populating it.
    pub fn show(&mut self, original_name: String, parent_dir: PathBuf, is_dir: bool) {
        let stem_len = std::path::Path::new(&original_name)
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .map_or_else(
                || original_name.chars().count(),
                |stem| stem.chars().count(),
            );
        self.new_name.set_text(original_name.clone());
        self.new_name.set_cursor(stem_len);
        self.original_name = original_name;
        self.parent_dir = parent_dir;
        self.show_overwrite_confirm = false;
        self.is_dir = is_dir;
        self.error = None;
        self.focused_button = 0;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.new_name.clear();
        self.original_name.clear();
        self.parent_dir = PathBuf::new();
        self.show_overwrite_confirm = false;
        self.is_dir = false;
        self.error = None;
        self.focused_button = 0;
    }
}

impl Default for RenameState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
