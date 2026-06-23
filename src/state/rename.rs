use std::path::PathBuf;

pub struct RenameState {
    pub is_visible: bool,
    pub new_name: String,
    pub cursor_position: usize,
    pub original_name: String,
    pub parent_dir: PathBuf,
    pub show_overwrite_confirm: bool,
    pub is_dir: bool,
    pub error: Option<String>,
    pub focused_button: usize,
    pub popup_area: ratatui::layout::Rect,
    pub button_areas: Vec<ratatui::layout::Rect>,
}

impl RenameState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            new_name: String::new(),
            cursor_position: 0,
            original_name: String::new(),
            parent_dir: PathBuf::new(),
            show_overwrite_confirm: false,
            is_dir: false,
            error: None,
            focused_button: 0,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.new_name.clear();
        self.cursor_position = 0;
        self.original_name.clear();
        self.parent_dir = PathBuf::new();
        self.show_overwrite_confirm = false;
        self.is_dir = false;
        self.error = None;
        self.focused_button = 0;
        self.popup_area = ratatui::layout::Rect::default();
        self.button_areas.clear();
    }
}

impl crate::state::PopupState for RenameState {
    fn reset(&mut self) {
        self.reset();
    }

    fn set_visible(&mut self, visible: bool) {
        self.is_visible = visible;
    }
}

impl Default for RenameState {
    fn default() -> Self {
        Self::new()
    }
}
