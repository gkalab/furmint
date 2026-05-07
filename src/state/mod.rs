pub mod bookmark;
pub mod confirmation;
mod conflict;
mod copy_move;
mod create_dir;
pub mod create_file;
mod delete;
mod drive_select;
mod empty_trash;
mod error;
mod file_viewer;
mod help;
mod quit;
mod remote_edit;
mod rename;
mod rename_tab;
pub mod ssh;

pub use bookmark::BookmarkState;
pub use confirmation::{ConfirmationAction, ConfirmationState};
pub use conflict::ConflictState;
pub use copy_move::{CopyMoveAction, CopyMoveState};
pub use create_dir::CreateDirectoryState;
pub use create_file::CreateFileState;
pub use delete::DeleteState;
pub use drive_select::DriveSelectState;
pub use empty_trash::EmptyTrashState;
pub use error::ErrorState;
pub use file_viewer::{FileViewerSearchState, FileViewerState, ImageLoadResult};
pub use help::HelpState;
pub use quit::QuitConfirmationState;
pub use remote_edit::RemoteEditState;
pub use rename::RenameState;
pub use rename_tab::RenameTabState;
pub use ssh::{SshConnectionState, SshPasswordState};

pub trait PopupState {
    fn reset(&mut self);
    fn set_visible(&mut self, visible: bool);
}
