use anyhow::Result;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub path: PathBuf,
    pub visit_count: u32,
    pub last_visited: u64, // Unix timestamp
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DirectoryHistory {
    // Context -> Path -> Entry
    pub entries: HashMap<String, HashMap<PathBuf, DirEntry>>,
    #[serde(skip)]
    pub cache_file: PathBuf,
}

impl DirectoryHistory {
    pub const MAX_LOCAL_HISTORY_ENTRIES: usize = 200;
    pub const MAX_REMOTE_HISTORY_ENTRIES: usize = 50;

    /// Create a new `DirectoryHistory` with the default cache file location
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be created or history cannot be loaded.
    pub fn new() -> Result<Self> {
        let cache_file = Self::get_cache_file_path()?;
        let mut history = Self {
            entries: HashMap::new(),
            cache_file,
        };
        history.load()?;
        Ok(history)
    }

    /// Get the OS-specific history file path
    ///
    /// # Errors
    ///
    /// Returns an error if the state directory cannot be determined or created.
    fn get_cache_file_path() -> Result<PathBuf> {
        let state_dir = crate::paths::state_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine state directory"))?;
        fs::create_dir_all(&state_dir)?;
        Ok(state_dir.join("dir_history.json"))
    }

    /// Record a visit to a directory within a specific context
    ///
    /// # Panics
    ///
    /// Panics if the system time is before the Unix epoch.
    pub fn record_visit(&mut self, context: &str, path: &Path) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let path = path.to_path_buf();
        let context_entries = self.entries.entry(context.to_string()).or_default();

        context_entries
            .entry(path.clone())
            .and_modify(|e| {
                e.visit_count += 1;
                e.last_visited = now;
            })
            .or_insert(DirEntry {
                path,
                visit_count: 1,
                last_visited: now,
            });
    }

    /// Get all directories sorted by score (frequency + recency) for a context
    ///
    /// # Panics
    ///
    /// Panics if the scores cannot be compared.
    #[must_use]
    pub fn get_sorted_dirs(&self, context: &str) -> Vec<PathBuf> {
        let Some(context_entries) = self.entries.get(context) else {
            return Vec::new();
        };

        let mut entries: Vec<_> = context_entries.values().collect();

        // Sort by score: combination of visit count and recency
        entries.sort_by(|a, b| {
            let score_a = Self::calculate_score(a);
            let score_b = Self::calculate_score(b);
            score_b.partial_cmp(&score_a).unwrap()
        });

        entries.iter().map(|e| e.path.clone()).collect()
    }

    /// Calculate a score for a directory based on visit count and recency
    fn calculate_score(entry: &DirEntry) -> f64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Age in days
        let age_seconds = now.saturating_sub(entry.last_visited);
        #[allow(clippy::cast_precision_loss)]
        let age_days = age_seconds as f64 / 86400.0; // casting is safe: only affects score precision, not correctness

        // Decay factor: recent visits are worth more
        // After 30 days, recency contributes very little
        let recency_score = (-age_days / 30.0).exp();

        // Combine visit count and recency
        // Visit count has more weight
        (f64::from(entry.visit_count) * 2.0) + (recency_score * 10.0)
    }

    /// Remove a specific directory entry from a context.
    ///
    /// Returns `true` if the entry existed and was removed.
    pub fn remove_entry(&mut self, context: &str, path: &Path) -> bool {
        if let Some(context_entries) = self.entries.get_mut(context) {
            context_entries.remove(path).is_some()
        } else {
            false
        }
    }

    /// Perform fuzzy search on directory paths within a context
    #[must_use]
    pub fn fuzzy_search(&self, context: &str, query: &str) -> Vec<(PathBuf, i64)> {
        if query.is_empty() {
            // Return all directories sorted by score
            return self
                .get_sorted_dirs(context)
                .into_iter()
                .map(|p| (p, 0))
                .collect();
        }

        let Some(context_entries) = self.entries.get(context) else {
            return Vec::new();
        };

        let matcher = SkimMatcherV2::default();
        let mut results: Vec<_> = context_entries
            .values()
            .filter_map(|entry| {
                let path_str = entry.path.to_string_lossy();
                matcher
                    .fuzzy_match(&path_str, query)
                    .map(|score| (entry.clone(), score))
            })
            .collect();

        // Sort by fuzzy match score (higher is better), then by history score
        results.sort_by(|a, b| match b.1.cmp(&a.1) {
            std::cmp::Ordering::Equal => {
                let score_a = Self::calculate_score(&a.0);
                let score_b = Self::calculate_score(&b.0);
                score_b
                    .partial_cmp(&score_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
            other => other,
        });

        results
            .into_iter()
            .map(|(entry, score)| (entry.path, score))
            .collect()
    }

    /// Load history from cache file
    ///
    /// # Errors
    ///
    /// Returns an error if the cache file cannot be read.
    pub fn load(&mut self) -> Result<()> {
        if !self.cache_file.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.cache_file)?;
        // If loading fails (e.g. old format), just start with empty history
        if let Ok(loaded) = serde_json::from_str::<DirectoryHistory>(&content) {
            self.entries = loaded.entries;
        }
        Ok(())
    }

    /// Save history to cache file
    ///
    /// # Errors
    ///
    /// Returns an error if the history cannot be serialized or written.
    pub fn save(&self) -> Result<()> {
        // Create a temporary struct for serialization
        #[derive(Serialize)]
        struct SaveData {
            entries: HashMap<String, HashMap<PathBuf, DirEntry>>,
        }

        let mut filtered_entries: HashMap<String, HashMap<PathBuf, DirEntry>> = HashMap::new();

        for (ctx, entries) in &self.entries {
            if ctx.starts_with("archive:") {
                continue;
            }

            let limit = if ctx == "local" {
                Self::MAX_LOCAL_HISTORY_ENTRIES
            } else {
                Self::MAX_REMOTE_HISTORY_ENTRIES
            };

            // Collect entries for this context and calculate scores
            let mut context_entries: Vec<_> = entries
                .iter()
                .map(|(path, entry)| (path, entry, Self::calculate_score(entry)))
                .collect();

            // Sort by score descending
            context_entries
                .sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

            // Take top N and rebuild the map for this context
            let mut pruned_map = HashMap::new();
            for (path, entry, _) in context_entries.into_iter().take(limit) {
                pruned_map.insert(path.clone(), entry.clone());
            }

            if !pruned_map.is_empty() {
                filtered_entries.insert(ctx.clone(), pruned_map);
            }
        }

        let data = SaveData {
            entries: filtered_entries,
        };

        let content = serde_json::to_string_pretty(&data)?;
        fs::write(&self.cache_file, content)?;
        Ok(())
    }
}
