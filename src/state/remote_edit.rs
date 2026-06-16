use std::path::PathBuf;
use std::sync::Arc;

pub struct RemoteEditState {
    pub is_visible: bool,
    pub temp_path: PathBuf,
    pub remote_path: PathBuf,
    pub filename: String,
    pub provider: Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
    pub original_checksum: [u8; 16],
    pub focused_button: usize,
}

impl RemoteEditState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            temp_path: PathBuf::new(),
            remote_path: PathBuf::new(),
            filename: String::new(),
            provider: Arc::new(crate::fs::fs_local::LocalFs::new()),
            original_checksum: [0; 16],
            focused_button: 0,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.temp_path = PathBuf::new();
        self.remote_path = PathBuf::new();
        self.filename.clear();
        self.provider = Arc::new(crate::fs::fs_local::LocalFs::new());
        self.original_checksum = [0; 16];
        self.focused_button = 0;
    }
}

impl Default for RemoteEditState {
    fn default() -> Self {
        Self::new()
    }
}
