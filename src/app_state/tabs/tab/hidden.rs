use super::Tab;

impl Tab {
    /// Reconcile the visible set with a new hidden-file setting.
    ///
    /// Idempotent: the visible set is always recomputed from `entries`, so this
    /// also repairs a stale [`Self::visible_indices`]. The tab's file filter is
    /// left untouched, so an active glob is re-evaluated against the new setting
    /// rather than being cleared.
    pub fn set_show_hidden(&mut self, show_hidden: bool) {
        self.show_hidden = show_hidden;
        self.rebuild_visible_preserving_cursor();

        // Never leave an invisible entry selected: operations act on `selected`
        // and would otherwise touch files that are not on screen. Collect first,
        // then mutate, so the `selected` read and write do not overlap.
        if !show_hidden {
            let deselect: Vec<usize> = (0..self.entries.len())
                .filter(|&i| self.entries[i].selected && !self.is_visible(i))
                .collect();
            for i in deselect {
                self.entries[i].selected = false;
            }
        }
    }

    /// Whether the entry at `index` passes the current file filter.
    ///
    /// `visible_indices` is kept in ascending order, so this is a binary search.
    #[must_use]
    pub fn is_visible(&self, index: usize) -> bool {
        self.visible_indices.binary_search(&index).is_ok()
    }

    /// Get the count of visible entries.
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.visible_indices.len()
    }

    /// Select all visible entries (files and directories) that pass the filter.
    /// ".." is never selected.
    pub fn select_all_visible(&mut self) {
        // `visible_indices` is ascending, so a merge walk over the entries keeps
        // this O(entries) instead of a membership test per entry.
        let mut visible = self.visible_indices.iter().copied().peekable();
        for (i, entry) in self.entries.iter_mut().enumerate() {
            while visible.peek().is_some_and(|&v| v < i) {
                visible.next();
            }
            if visible.peek() != Some(&i) || entry.name == ".." {
                continue;
            }
            entry.selected = true;
        }
    }

    /// Get the count of visible files (excludes directories and "..")
    #[must_use]
    pub fn visible_file_count(&self) -> usize {
        self.visible_indices
            .iter()
            .filter(|&&i| !self.entries[i].is_dir)
            .count()
    }

    pub fn toggle_selection(&mut self) {
        let Some(index) = self.cursor_entry_index() else {
            return;
        };
        if let Some(entry) = self.entries.get_mut(index).filter(|e| e.name != "..") {
            entry.selected = !entry.selected;
        }
    }

    /// Select all currently visible files and directories.
    ///
    /// ".." is never selected. The result is independent of whether hidden
    /// files are shown, since the visible set already reflects the current
    /// hidden-file setting and file filter.
    pub fn select_all(&mut self) {
        self.select_all_visible();
    }
}
