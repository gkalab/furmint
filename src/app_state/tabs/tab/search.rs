use std::time::Duration;

use super::Tab;

pub const SEARCH_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Default)]
pub struct IncrementalSearch {
    pub buffer: String,
    pub last_type_time: Option<std::time::Instant>,
    pub matching_indices: Vec<usize>,
    pub position: usize,
    pub highlights: std::collections::HashMap<usize, Vec<usize>>,
}

impl IncrementalSearch {
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.buffer.is_empty()
            && self
                .last_type_time
                .is_some_and(|t| t.elapsed() < SEARCH_TIMEOUT)
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.last_type_time = None;
        self.matching_indices.clear();
        self.position = 0;
        self.highlights.clear();
    }
}

impl Tab {
    #[must_use]
    pub fn is_search_active(&self) -> bool {
        self.search.is_active()
    }

    pub fn reset_search(&mut self) {
        self.search.reset();
    }

    pub fn apply_search_highlights(&mut self) {
        if self.search.buffer.is_empty() {
            self.search.highlights.clear();
            self.search.matching_indices.clear();
            return;
        }

        let query = self.search.buffer.to_lowercase();
        self.search.matching_indices.clear();
        self.search.highlights.clear();

        // 1. Prefix matches
        for (i, entry) in self.entries.iter().enumerate() {
            if !self.is_visible(i) {
                continue;
            }
            if entry.name.to_lowercase().starts_with(&query) {
                self.search.matching_indices.push(i);
                // For prefix matches, highlight the prefix
                let mut matches = Vec::new();
                for j in 0..self.search.buffer.chars().count() {
                    matches.push(j);
                }
                self.search.highlights.insert(i, matches);
            }
        }

        // 2. Fuzzy matches (if no prefix matches or to supplement)
        if self.search.matching_indices.is_empty() {
            use fuzzy_matcher::FuzzyMatcher;
            use fuzzy_matcher::skim::SkimMatcherV2;
            let matcher = SkimMatcherV2::default();

            for (i, entry) in self.entries.iter().enumerate() {
                if !self.is_visible(i) {
                    continue;
                }
                if let Some((_, indices)) =
                    matcher.fuzzy_indices(&entry.name.to_lowercase(), &query)
                {
                    self.search.matching_indices.push(i);
                    self.search.highlights.insert(i, indices);
                }
            }
        }
    }
}
