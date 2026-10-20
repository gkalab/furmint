//! Tabs and the manager that owns them.
//!
//! `Tab` is split by concern: the type, its constructors and navigation live in
//! [`tab`], while sorting, file filtering, incremental search and the hidden-file
//! policy each get their own submodule. [`manager`] holds `TabManager`.

mod manager;
mod tab;

pub use manager::*;
pub use tab::*;

#[cfg(test)]
mod tests;
