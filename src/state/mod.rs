// State module - contains popup and UI state structs
// Extracted from app.rs for better organization

mod conflict;
mod copy_move;
mod create_dir;
mod create_file;
mod delete;
mod drive_select;
mod empty_trash;
mod error;
mod file_viewer;
mod help;
mod quit;
mod rename;

pub use conflict::ConflictState;
pub use copy_move::{CopyMoveAction, CopyMoveState};
pub use create_dir::CreateDirectoryState;
pub use create_file::CreateFileState;
pub use delete::DeleteState;
pub use drive_select::DriveSelectState;
pub use empty_trash::EmptyTrashState;
pub use error::ErrorState;
pub use file_viewer::FileViewerState;
pub use help::HelpState;
pub use quit::QuitConfirmationState;
pub use rename::RenameState;
