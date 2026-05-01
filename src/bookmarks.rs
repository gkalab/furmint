use anyhow::Result;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkEntry {
    pub path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct BookmarkStore {
    pub entries: Vec<BookmarkEntry>,
    #[serde(skip)]
    pub path: PathBuf,
}

impl BookmarkStore {
    /// Create a new `BookmarkStore` with the default file location
    ///
    /// # Errors
    ///
    /// Returns an error if the config directory cannot be determined or created.
    pub fn new() -> Result<Self> {
        let path = Self::get_storage_path()?;
        let mut store = Self {
            entries: Vec::new(),
            path,
        };
        store.load()?;
        Ok(store)
    }

    fn get_storage_path() -> Result<PathBuf> {
        let state_dir = crate::paths::state_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine state directory"))?;
        fs::create_dir_all(&state_dir)?;
        Ok(state_dir.join("bookmarks.json"))
    }

    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn test_default() -> Self {
        Self {
            entries: Vec::new(),
            path: PathBuf::from("/tmp/bookmarks_test.json"),
        }
    }

    pub fn add(&mut self, path: PathBuf) -> bool {
        if self.entries.iter().any(|e| e.path == path) {
            return false;
        }
        self.entries.push(BookmarkEntry { path });
        let _ = self.save();
        true
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.entries.remove(index);
            let _ = self.save();
            true
        } else {
            false
        }
    }

    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.entries.iter().any(|e| e.path == path)
    }

    #[must_use]
    pub fn fuzzy_search(&self, query: &str) -> Vec<PathBuf> {
        if query.is_empty() {
            let mut paths: Vec<_> = self.entries.iter().map(|e| e.path.clone()).collect();
            paths.sort();
            return paths;
        }

        let matcher = SkimMatcherV2::default();
        let mut results: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let path_str = entry.path.to_string_lossy();
                matcher
                    .fuzzy_match(&path_str, query)
                    .map(|score| (entry.path.clone(), score))
            })
            .collect();

        results.sort_by(|a, b| b.1.cmp(&a.1));
        results.into_iter().map(|(path, _)| path).collect()
    }

    /// Loads bookmarks from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the bookmarks cannot be read from disk.
    pub fn load(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.path)?;
        if let Ok(loaded) = serde_json::from_str::<Vec<BookmarkEntry>>(&content) {
            self.entries = loaded;
        }
        Ok(())
    }

    /// Saves bookmarks to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the bookmarks cannot be saved to disk.
    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.entries)?;
        fs::write(&self.path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_bookmark_store_basic() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("bookmarks.json");
        let mut store = BookmarkStore {
            entries: Vec::new(),
            path: file_path,
        };

        let p1 = PathBuf::from("/home/user/docs");
        let p2 = PathBuf::from("/var/log");

        assert!(store.add(p1.clone()));
        assert!(!store.add(p1.clone())); // Duplicate
        assert!(store.add(p2.clone()));
        assert_eq!(store.entries.len(), 2);

        assert!(store.contains(&p1));
        assert!(store.contains(&p2));

        let results = store.fuzzy_search("log");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], p2);

        assert!(store.remove(0));
        assert_eq!(store.entries.len(), 1);
        assert!(!store.contains(&p1));

        Ok(())
    }
}
