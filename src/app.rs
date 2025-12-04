use std::path::PathBuf;
use crate::fs_ops::FileEntry;

#[derive(Clone)]
pub struct PanelState {
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub history: Vec<PathBuf>,
    pub history_index: usize,
    pub error: Option<String>,
    // For incremental search
    pub typed_buffer: String,
    pub last_type_time: Option<std::time::Instant>,
}

#[derive(PartialEq)]
pub enum PanelSide {
    Left,
    Right,
}

pub struct AppState {
    pub left: PanelState,
    pub right: PanelState,
    pub active: PanelSide,

}
