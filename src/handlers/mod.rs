//! Main event handler dispatcher and re-exports for categories

pub mod navigation;
pub use navigation::*;

pub mod tabs;
pub use tabs::*;

pub mod popup_fuzzy;
pub use popup_fuzzy::*;
pub mod popup_rename;
pub use popup_rename::*;
pub mod popup_delete;
pub use popup_delete::*;

pub mod popup_copy_move;
pub use popup_copy_move::*;
pub mod popup_conflict;
pub use popup_conflict::*;
pub mod popup_error;
pub use popup_error::*;
pub mod popup_create;
pub use popup_create::*;
pub mod popup_misc;
pub use popup_misc::*;
pub mod file_viewer;
pub use file_viewer::*;
pub mod editor;
pub use editor::*;
pub mod terminal;
pub use terminal::*;
pub mod file_ops;
