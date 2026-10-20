use crate::fs::utils::FileEntry;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::Tab;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortSettings {
    pub column: SortColumn,
    pub direction: SortDirection,
}

impl Default for SortSettings {
    fn default() -> Self {
        Self {
            column: SortColumn::Name,
            direction: SortDirection::Ascending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortColumn {
    Name,
    Extension,
    Date,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl Tab {
    /// Sorts `entries` in place and rebuilds the visible set.
    ///
    /// Entry order defines the order of [`Self::visible_indices`], so the two can
    /// only be kept consistent here: any reorder invalidates the index list.
    pub fn sort_entries(&mut self) {
        let mut entries = std::mem::take(&mut self.entries);
        self.sort_entries_list(&mut entries);
        self.entries = entries;
        self.recompute_visible_indices();
    }

    pub fn sort_entries_list(&self, entries: &mut [FileEntry]) {
        entries.sort_by(|a, b| {
            // ".." always first
            if a.name == ".." {
                return std::cmp::Ordering::Less;
            }
            if b.name == ".." {
                return std::cmp::Ordering::Greater;
            }

            // Directories before files
            if a.is_dir && !b.is_dir {
                return std::cmp::Ordering::Less;
            }
            if !a.is_dir && b.is_dir {
                return std::cmp::Ordering::Greater;
            }

            // Specific directory sorting logic
            if a.is_dir && b.is_dir {
                match self.sort.column {
                    SortColumn::Name => {
                        let ord = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                        return if self.sort.direction == SortDirection::Descending {
                            ord.reverse()
                        } else {
                            ord
                        };
                    }
                    SortColumn::Date => {
                        let date_a = a.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        let date_b = b.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        let ord = date_a
                            .cmp(&date_b)
                            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                        return if self.sort.direction == SortDirection::Descending {
                            ord.reverse()
                        } else {
                            ord
                        };
                    }
                    SortColumn::Size => {
                        // Get effective size for sorting: use calculated size if available, otherwise use raw size
                        let size_a = if a.is_dir {
                            self.get_dir_size(&self.current_dir.join(&a.name))
                                .map_or(a.size, Some)
                        } else {
                            a.size
                        };
                        let size_b = if b.is_dir {
                            self.get_dir_size(&self.current_dir.join(&b.name))
                                .map_or(b.size, Some)
                        } else {
                            b.size
                        };
                        let ord = size_a
                            .cmp(&size_b)
                            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                        return if self.sort.direction == SortDirection::Descending {
                            ord.reverse()
                        } else {
                            ord
                        };
                    }
                    SortColumn::Extension => {
                        // Extension: Directories always Name Ascending
                        return a.name.to_lowercase().cmp(&b.name.to_lowercase());
                    }
                }
            }

            let ord = match self.sort.column {
                SortColumn::Name => {
                    // Case-insensitive natural sort would be better, but let's start with basic
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                }
                SortColumn::Extension => {
                    let ext_a = Path::new(&a.name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let ext_b = Path::new(&b.name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    ext_a
                        .cmp(&ext_b)
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
                SortColumn::Size => a
                    .size
                    .cmp(&b.size)
                    .then(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
                SortColumn::Date => {
                    let date_a = a.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    let date_b = b.modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    date_a
                        .cmp(&date_b)
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
            };

            if self.sort.direction == SortDirection::Descending {
                ord.reverse()
            } else {
                ord
            }
        });
    }

    pub(super) fn entries_equal_ignoring_selection(&self, new_entries: &[FileEntry]) -> bool {
        if self.entries.len() != new_entries.len() {
            return false;
        }

        for (old, new) in self.entries.iter().zip(new_entries.iter()) {
            if old.name != new.name
                || old.is_dir != new.is_dir
                || old.is_symlink != new.is_symlink
                || old.size != new.size
                || old.modified != new.modified
                || old.attributes != new.attributes
            {
                return false;
            }
        }

        true
    }

    pub fn handle_sort(&mut self, column: SortColumn) {
        if self.sort.column == column {
            self.sort.direction = match self.sort.direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort.column = column;
            self.sort.direction = match column {
                SortColumn::Name | SortColumn::Extension => SortDirection::Ascending,
                SortColumn::Size | SortColumn::Date => SortDirection::Descending,
            };
        }
        let current_selection = self.current_entry().map(|e| e.name.clone());
        self.sort_entries();
        // Try to keep cursor on the same item, but only if it is still listed.
        if let Some(row) = current_selection
            .and_then(|name| self.entries.iter().position(|e| e.name == name))
            .and_then(|idx| self.entry_index_to_visible_row(idx))
        {
            self.cursor = row;
        }
    }
}
