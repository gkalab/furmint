//! Event handlers and the app-state conventions they follow.
//!
//! # Where behaviour lives
//!
//! Decide by *what is mutated*, not by who calls it:
//!
//! - Anything that mutates [`crate::app::AppState`]'s own fields is a method on
//!   `AppState`, on the state type that owns the field, or on `Popups` — callers
//!   go through it instead of writing the fields. A single transition that must
//!   preserve an invariant is a method.
//! - Multi-step flows — read state, await a provider, emit events — stay free
//!   functions in this module and call those methods.
//!
//! The boundary test: a single transition that has to preserve an invariant is
//! a method; a sequence of calls and awaits is a handler.
//!
//! For popups this means "open popup X with data" is one method on the popup's
//! state type (e.g. [`crate::state::HostKeyState::show`]) that sets `is_visible`
//! itself, so visibility and contents can never be set separately. The related
//! key/mouse handler registry lives in `declare_popups!` in [`crate::app`].

pub mod event_dispatch;
pub mod main_handler;
pub mod mouse;
pub mod navigation;

pub mod tabs;

pub mod popup_bookmark;
pub mod popup_conflict;
pub mod popup_copy_move;
pub mod popup_create;
pub mod popup_delete;
pub mod popup_drive_select;
pub mod popup_empty_trash;
pub mod popup_error;
pub mod popup_fuzzy;
pub mod popup_help;
pub mod popup_host_key;
pub mod popup_quit;
pub mod popup_rename;
pub mod popup_rename_tab;
pub mod popup_ssh;
pub mod popup_task_manager;

pub mod clipboard_utils;
pub mod editor;
pub mod external;
pub mod file_filter;
pub mod file_viewer;
pub mod input;
pub mod input_utils;
pub mod popup_utils;
pub mod suspended_ui;
pub mod terminal;
pub mod toggle_hidden;
