use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;

use crate::handlers::editor::TempFileGuard;

pub struct RemoteEditState {
    pub is_visible: bool,
    pub temp_guard: Option<TempFileGuard>,
    pub editor_child: Option<Child>,
    pub remote_path: PathBuf,
    pub filename: String,
    pub provider: Arc<dyn crate::fs::provider::FileSystemProvider>,
    pub original_checksum: [u8; 16],
    pub focused_button: usize,
    pub popup_area: ratatui::layout::Rect,
    pub button_areas: Vec<ratatui::layout::Rect>,
}

impl RemoteEditState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            temp_guard: None,
            editor_child: None,
            remote_path: PathBuf::new(),
            filename: String::new(),
            provider: Arc::new(crate::fs::local::LocalFs::new()),
            original_checksum: [0; 16],
            focused_button: 0,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.temp_guard = None;
        self.editor_child = None;
        self.remote_path = PathBuf::new();
        self.filename.clear();
        self.provider = Arc::new(crate::fs::local::LocalFs::new());
        self.original_checksum = [0; 16];
        self.focused_button = 0;
        self.popup_area = ratatui::layout::Rect::default();
        self.button_areas.clear();
    }

    /// Closes the popup, guaranteeing the temp file is removed.
    ///
    /// If a detached editor is still running, removal is deferred until it
    /// exits so the file is not unlinked from under it.
    pub fn close(&mut self) {
        let child = self.editor_child.take();
        let guard = self.temp_guard.take();
        if let Some(mut child) = child
            && matches!(child.try_wait(), Ok(None))
        {
            std::thread::spawn(move || {
                let _ = child.wait();
                drop(guard);
            });
        } else {
            drop(guard);
        }
        self.reset();
    }
}

impl Default for RemoteEditState {
    fn default() -> Self {
        Self::new()
    }
}
