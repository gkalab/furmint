use anyhow::Result;
use directories::ProjectDirs;
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

#[derive(Debug, Serialize, Deserialize)]
pub struct DirectoryHistory {
    entries: HashMap<PathBuf, DirEntry>,
    #[serde(skip)]
    cache_file: PathBuf,
}

impl DirectoryHistory {
    /// Create a new `DirectoryHistory` with the default cache file location
    pub fn new() -> Result<Self> {
        let cache_file = Self::get_cache_file_path()?;
        let mut history = Self {
            entries: HashMap::new(),
            cache_file,
        };
        history.load()?;
        Ok(history)
    }

    /// Get the OS-specific cache file path
    fn get_cache_file_path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("org", "fm", "fm")
            .ok_or_else(|| anyhow::anyhow!("Could not determine cache directory"))?;
        let cache_dir = proj_dirs.cache_dir();
        fs::create_dir_all(cache_dir)?;
        Ok(cache_dir.join("dir_history.json"))
    }

    /// Record a visit to a directory
    pub fn record_visit(&mut self, path: &Path) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let path = path.to_path_buf();
        self.entries
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

    /// Get all directories sorted by score (frequency + recency)
    pub fn get_sorted_dirs(&self) -> Vec<PathBuf> {
        let mut entries: Vec<_> = self.entries.values().collect();

        // Sort by score: combination of visit count and recency
        entries.sort_by(|a, b| {
            let score_a = self.calculate_score(a);
            let score_b = self.calculate_score(b);
            score_b.partial_cmp(&score_a).unwrap()
        });

        entries.iter().map(|e| e.path.clone()).collect()
    }

    /// Calculate a score for a directory based on visit count and recency
    fn calculate_score(&self, entry: &DirEntry) -> f64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Age in days
        let age_seconds = now.saturating_sub(entry.last_visited);
        let age_days = age_seconds as f64 / 86400.0;

        // Decay factor: recent visits are worth more
        // After 30 days, recency contributes very little
        let recency_score = (-age_days / 30.0).exp();

        // Combine visit count and recency
        // Visit count has more weight
        (f64::from(entry.visit_count) * 2.0) + (recency_score * 10.0)
    }

    /// Perform fuzzy search on directory paths
    pub fn fuzzy_search(&self, query: &str) -> Vec<(PathBuf, i64)> {
        if query.is_empty() {
            // Return all directories sorted by score
            return self.get_sorted_dirs().into_iter().map(|p| (p, 0)).collect();
        }

        let matcher = SkimMatcherV2::default();
        let mut results: Vec<_> = self
            .entries
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
                let score_a = self.calculate_score(&a.0);
                let score_b = self.calculate_score(&b.0);
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
    pub fn load(&mut self) -> Result<()> {
        if !self.cache_file.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.cache_file)?;
        let loaded: DirectoryHistory = serde_json::from_str(&content)?;
        self.entries = loaded.entries;
        Ok(())
    }

    /// Save history to cache file
    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self)?;
        fs::write(&self.cache_file, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_visit() {
        let mut history = DirectoryHistory {
            entries: HashMap::new(),
            cache_file: PathBuf::from("/tmp/test.json"),
        };

        let path = PathBuf::from("/home/user");
        history.record_visit(&path);
        assert_eq!(history.entries.get(&path).unwrap().visit_count, 1);

        history.record_visit(&path);
        assert_eq!(history.entries.get(&path).unwrap().visit_count, 2);
    }

    #[test]
    fn test_fuzzy_search() {
        let mut history = DirectoryHistory {
            entries: HashMap::new(),
            cache_file: PathBuf::from("/tmp/test.json"),
        };

        history.record_visit(&PathBuf::from("/home/user"));
        history.record_visit(&PathBuf::from("/usr/local"));
        history.record_visit(&PathBuf::from("/var/log"));

        let results = history.fuzzy_search("hm");
        assert!(!results.is_empty());
        assert!(
            results
                .iter()
                .any(|(p, _)| p.to_string_lossy().contains("home"))
        );
    }
    #[test]
    fn test_fuzzy_search_sorting() {
        let mut history = DirectoryHistory {
            entries: HashMap::new(),
            cache_file: PathBuf::from("/tmp/test_fuzzy_sorting.json"),
        };

        // path_a: visited 10 times (high score)
        let path_a = PathBuf::from("/home/user/documents");
        for _ in 0..10 {
            history.record_visit(&path_a);
        }

        // path_b: visited 1 time (low score)
        let path_b = PathBuf::from("/home/user/downloads");
        history.record_visit(&path_b);

        // search for "do" - both match
        // expected: path_a comes first because 10 visits > 1 visit
        // even if fuzzy match score is similar or identical for "do"
        let results = history.fuzzy_search("do");

        // Find positions of both paths
        let pos_a = results.iter().position(|(p, _)| p == &path_a);
        let pos_b = results.iter().position(|(p, _)| p == &path_b);

        assert!(pos_a.is_some(), "path_a should be in results");
        assert!(pos_b.is_some(), "path_b should be in results");

        // Assert path_a comes before path_b
        assert!(
            pos_a.unwrap() < pos_b.unwrap(),
            "Highly visited path should come before less visited path"
        );
    }
}
