use super::entry::{attributes_char_style, style_for_entry, style_for_icon};
use super::layout::{attributes_col_width, name_col_width_for};
use super::status::get_task_progress_spans;
use crate::fs::utils::FileEntry;
use crate::theme::{Rgb, ThemePalette};
use ratatui::prelude::{Color, Style};
use std::path::Path;

mod entry;
mod layout;
mod status;
