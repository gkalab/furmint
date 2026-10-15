use anyhow::Result;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// serde requires skip_serializing_if predicates to take a reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkEntry {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_port: Option<u16>,
    /// `true` when `path` points at an archive file that should be opened in an
    /// archive tab rather than navigated to as a directory.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_archive: bool,
}

impl BookmarkEntry {
    #[must_use]
    pub fn is_remote(&self) -> bool {
        self.ssh_host.is_some()
    }

    #[must_use]
    pub fn display_string(&self) -> String {
        match (&self.ssh_user, &self.ssh_host) {
            (Some(user), Some(host)) => {
                let port = self.ssh_port.unwrap_or(22);
                if port == 22 {
                    format!("{}@{}:{}", user, host, self.path.display())
                } else {
                    format!("{}@{}:{}:{}", user, host, port, self.path.display())
                }
            }
            _ => self.path.display().to_string(),
        }
    }
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

    #[cfg(test)]
    #[must_use]
    pub fn test_default() -> Self {
        Self {
            entries: Vec::new(),
            path: std::env::temp_dir().join("bookmarks_test.json"),
        }
    }

    /// Adds a local bookmark.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be persisted to disk.
    #[cfg(test)]
    pub fn add(&mut self, path: PathBuf) -> Result<bool> {
        self.add_with_ssh(path, None, None, None)
    }

    /// Adds a bookmark (optionally remote). Returns `Ok(true)` if it was added,
    /// `Ok(false)` if an identical bookmark already exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be persisted to disk.
    #[cfg(test)]
    pub fn add_with_ssh(
        &mut self,
        path: PathBuf,
        ssh_user: Option<String>,
        ssh_host: Option<String>,
        ssh_port: Option<u16>,
    ) -> Result<bool> {
        self.add_entry(BookmarkEntry {
            path,
            ssh_user,
            ssh_host,
            ssh_port,
            is_archive: false,
        })
    }

    /// Adds a fully specified bookmark. Returns `Ok(true)` if it was added,
    /// `Ok(false)` if an identical bookmark already exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be persisted to disk.
    pub fn add_entry(&mut self, entry: BookmarkEntry) -> Result<bool> {
        let already = self.entries.iter().any(|e| {
            e.path == entry.path
                && e.ssh_user == entry.ssh_user
                && e.ssh_host == entry.ssh_host
                && e.ssh_port == entry.ssh_port
                && e.is_archive == entry.is_archive
        });
        if already {
            return Ok(false);
        }
        self.entries.push(entry);
        self.save()?;
        Ok(true)
    }

    /// Removes the bookmark at `index`. Returns `Ok(true)` if a bookmark was
    /// removed, `Ok(false)` if the index is out of range.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be persisted to disk.
    pub fn remove(&mut self, index: usize) -> Result<bool> {
        if index < self.entries.len() {
            self.entries.remove(index);
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Remove a bookmark by `(path, is_remote)`. A local and a remote bookmark
    /// can share the same path, so the path alone is not a unique key.
    /// Returns `Ok(true)` if found and removed, `Ok(false)` if not.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be persisted to disk.
    pub fn remove_by_path(&mut self, path: &Path, is_remote: bool) -> Result<bool> {
        if let Some(idx) = self
            .entries
            .iter()
            .position(|e| e.path == path && e.is_remote() == is_remote)
        {
            self.entries.remove(idx);
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Returns the full entries matching `query`, ordered by fuzzy score
    /// (or by display string when the query is empty).
    #[must_use]
    pub fn fuzzy_search_entries(&self, query: &str) -> Vec<BookmarkEntry> {
        if query.is_empty() {
            let mut entries: Vec<_> = self.entries.clone();
            entries.sort_by_key(BookmarkEntry::display_string);
            return entries;
        }

        let matcher = SkimMatcherV2::default();
        let mut results: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let path_str = entry.display_string();
                matcher
                    .fuzzy_match(&path_str, query)
                    .map(|score| (entry.clone(), score))
            })
            .collect();

        results.sort_by_key(|b| std::cmp::Reverse(b.1));
        results.into_iter().map(|(entry, _)| entry).collect()
    }

    #[cfg(test)]
    #[must_use]
    pub fn find_entry(&self, path: &Path, is_remote: bool) -> Option<&BookmarkEntry> {
        self.entries
            .iter()
            .find(|e| e.path == path && e.ssh_host.is_some() == is_remote)
    }

    #[cfg(test)]
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.entries.iter().any(|e| e.path == path)
    }

    /// Returns the paths of [`Self::fuzzy_search_entries`], in the same order.
    #[cfg(test)]
    #[must_use]
    pub fn fuzzy_search(&self, query: &str) -> Vec<PathBuf> {
        self.fuzzy_search_entries(query)
            .into_iter()
            .map(|entry| entry.path)
            .collect()
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
        crate::paths::atomic_write(&self.path, &content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
