use crate::fs::utils::FileEntry;

use super::Tab;

impl Tab {
    /// Set a file name filter. The pattern is a glob matched against entry names,
    /// implicitly wrapped in `*...*` so a plain substring matches names containing
    /// it. The filter only applies to files: directories (including "..") are
    /// always visible.
    ///
    /// # Errors
    ///
    /// Returns `globset::Error` if the pattern is not a valid glob.
    pub fn set_file_filter(&mut self, pattern: Option<&str>) -> Result<(), globset::Error> {
        self.filter.set(pattern)?;
        self.rebuild_visible_preserving_cursor();
        Ok(())
    }

    pub fn clear_file_filter(&mut self) {
        self.filter.clear();
        self.rebuild_visible_preserving_cursor();
    }

    pub fn init_file_filter(&mut self) {
        self.filter.previous_filter = self.filter.applied.clone();
        self.filter.pattern.clear();
        self.filter.cursor_position = 0;
        self.filter.active = true;
    }

    pub fn apply_file_filter(&mut self) {
        let pattern = self.filter.pattern.trim().to_string();
        let _ = self.set_file_filter(Some(&pattern));
    }

    pub fn confirm_file_filter(&mut self) {
        let pattern = self.filter.pattern.trim().to_string();
        if pattern.is_empty() {
            self.reset_file_filter_state();
            self.clear_file_filter();
        } else if crate::state::compile_glob(&crate::state::effective_glob(&pattern)).is_ok() {
            let _ = self.set_file_filter(Some(&pattern));
            self.reset_file_filter_state();
        }
    }

    fn reset_file_filter_state(&mut self) {
        self.filter.active = false;
        self.filter.pattern.clear();
        self.filter.cursor_position = 0;
        self.filter.previous_filter = None;
    }

    pub fn cancel_file_filter(&mut self) {
        let previous = self.filter.previous_filter.take();
        self.filter.reset();
        match previous {
            Some(ref p) if !p.is_empty() => {
                let _ = self.set_file_filter(Some(p));
            }
            _ => {
                self.clear_file_filter();
            }
        }
    }

    /// Whether `entry` survives the hidden-file setting.
    ///
    /// This is the hidden gate only: the file filter is deliberately not
    /// consulted, so the gate always runs first and an active glob can never
    /// surface a hidden entry. `..` is a navigation entry rather than a hidden
    /// one, so it always survives.
    #[must_use]
    pub fn entry_is_listed(&self, entry: &FileEntry) -> bool {
        self.show_hidden || entry.name == ".." || !entry.is_hidden(self.provider.unix_attributes())
    }

    /// The number of files that exist under the current hidden-file setting,
    /// ignoring any active file filter.
    ///
    /// This is the honest denominator for the `Filter: <pattern> <visible>/<total>
    /// files` status label and the file count in the plain status line, since it
    /// only counts files that are actually listed.
    #[must_use]
    pub fn listed_file_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| !e.is_dir && self.entry_is_listed(e))
            .count()
    }

    /// Recompute `visible_indices` from entries based on the hidden-file setting
    /// and the current file filter.
    ///
    /// The hidden gate is applied first, so hidden files and hidden directories
    /// (including ".." excepted) never reach the listing. Of what remains,
    /// directories are always included; files are included only if they match the
    /// glob (or if no filter is active).
    ///
    /// Also adjusts `cursor` to ensure it points to a visible entry.
    pub fn recompute_visible_indices(&mut self) {
        let indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                if !self.entry_is_listed(e) {
                    None
                } else if e.is_dir {
                    Some(i)
                } else if let Some(ref glob) = self.filter.glob {
                    if glob.is_match(e.name.as_str()) {
                        Some(i)
                    } else {
                        None
                    }
                } else {
                    Some(i)
                }
            })
            .collect();
        self.visible_indices = indices;

        // Keep the cursor inside the visible listing. When nothing is visible
        // there is no row to land on, so pin it to 0 rather than leaving it
        // pointing past the end.
        if self.visible_indices.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.visible_indices.len() {
            self.cursor = self.visible_indices.len() - 1;
        }
    }

    /// Rebuild the visible set while keeping the cursor on the entry it was on,
    /// if that entry survives the new restriction. When the entry is withheld the
    /// cursor lands on the nearest visible entry above it (else the first
    /// visible one), and an empty listing pins it to `0`.
    ///
    /// Used when the restriction itself changes (the file filter, the hidden-file
    /// setting): the cursor is a row in the visible listing, so the same row would
    /// otherwise name a different entry after the rebuild.
    pub(super) fn rebuild_visible_preserving_cursor(&mut self) {
        let previous_entry = self.cursor_entry_index();
        self.recompute_visible_indices();
        let Some(entry_index) = previous_entry else {
            return;
        };
        // Keep the cursor on the same entry when it survives the new restriction.
        if let Some(row) = self.entry_index_to_visible_row(entry_index) {
            self.cursor = row;
            return;
        }
        // Otherwise land on the nearest visible entry above it, else the first.
        if !self.visible_indices.is_empty() {
            let above = self.visible_indices.partition_point(|&i| i < entry_index);
            self.cursor = above.saturating_sub(1);
        }
    }
}
