use anyhow::Result;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkEntry {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_port: Option<u16>,
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

    #[cfg(any(test, feature = "test-utils"))]
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
    pub fn add(&mut self, path: PathBuf) -> Result<bool> {
        self.add_with_ssh(path, None, None, None)
    }

    /// Adds a bookmark (optionally remote). Returns `Ok(true)` if it was added,
    /// `Ok(false)` if an identical bookmark already exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be persisted to disk.
    pub fn add_with_ssh(
        &mut self,
        path: PathBuf,
        ssh_user: Option<String>,
        ssh_host: Option<String>,
        ssh_port: Option<u16>,
    ) -> Result<bool> {
        let already = self.entries.iter().any(|e| {
            e.path == path
                && e.ssh_user == ssh_user
                && e.ssh_host == ssh_host
                && e.ssh_port == ssh_port
        });
        if already {
            return Ok(false);
        }
        self.entries.push(BookmarkEntry {
            path,
            ssh_user,
            ssh_host,
            ssh_port,
        });
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

    #[must_use]
    pub fn find_entry(&self, path: &Path, is_remote: bool) -> Option<&BookmarkEntry> {
        self.entries
            .iter()
            .find(|e| e.path == path && e.ssh_host.is_some() == is_remote)
    }

    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.entries.iter().any(|e| e.path == path)
    }

    /// Returns the paths of [`Self::fuzzy_search_entries`], in the same order.
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

        assert!(store.add(p1.clone()).unwrap());
        assert!(!store.add(p1.clone()).unwrap()); // Duplicate
        assert!(store.add(p2.clone()).unwrap());
        assert_eq!(store.entries.len(), 2);

        assert!(store.contains(&p1));
        assert!(store.contains(&p2));

        let results = store.fuzzy_search("log");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], p2);

        assert!(store.remove(0).unwrap());
        assert_eq!(store.entries.len(), 1);
        assert!(!store.contains(&p1));

        Ok(())
    }

    #[test]
    fn test_remove_by_path() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("bookmarks.json");
        let mut store = BookmarkStore {
            entries: Vec::new(),
            path: file_path,
        };

        let p1 = PathBuf::from("/home/user/docs");
        let p2 = PathBuf::from("/var/log");
        store.add(p1.clone()).unwrap();
        store.add(p2.clone()).unwrap();

        assert!(store.remove_by_path(&p1, false).unwrap());
        assert_eq!(store.entries.len(), 1);
        assert!(!store.contains(&p1));
        assert!(store.contains(&p2));

        // Removing non-existent path returns false
        assert!(!store.remove_by_path(&p1, false).unwrap());
        assert!(
            !store
                .remove_by_path(&PathBuf::from("/does/not/exist"), false)
                .unwrap()
        );

        Ok(())
    }

    #[test]
    fn test_remove_by_path_distinguishes_local_and_remote() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("bookmarks.json");
        let mut store = BookmarkStore {
            entries: Vec::new(),
            path: file_path,
        };

        let path = PathBuf::from("/home/user/src");
        // Local and remote bookmarks share the same path.
        store.add(path.clone()).unwrap();
        store
            .add_with_ssh(
                path.clone(),
                Some("u".to_string()),
                Some("host".to_string()),
                None,
            )
            .unwrap();
        assert_eq!(store.entries.len(), 2);

        // Removing the local one must leave the remote one intact.
        assert!(store.remove_by_path(&path, false).unwrap());
        assert_eq!(store.entries.len(), 1);
        assert!(store.find_entry(&path, true).is_some());
        assert!(store.find_entry(&path, false).is_none());

        // And vice-versa.
        assert!(store.remove_by_path(&path, true).unwrap());
        assert_eq!(store.entries.len(), 0);

        Ok(())
    }

    #[test]
    fn test_remote_and_local_distinct() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("bookmarks.json");
        let mut store = BookmarkStore {
            entries: Vec::new(),
            path: file_path,
        };

        let path = PathBuf::from("/home/user/src");
        // Local bookmark
        assert!(store.add(path.clone()).unwrap());
        // Remote bookmark with the same path is distinct
        assert!(
            store
                .add_with_ssh(
                    path.clone(),
                    Some("someuser".to_string()),
                    Some("host".to_string()),
                    None,
                )
                .unwrap()
        );
        assert!(
            store
                .add_with_ssh(
                    path.clone(),
                    Some("otheruser".to_string()),
                    Some("host".to_string()),
                    Some(2222),
                )
                .unwrap()
        );
        assert_eq!(store.entries.len(), 3);

        // Dedup: same user/host/path rejected
        assert!(
            !store
                .add_with_ssh(
                    path.clone(),
                    Some("someuser".to_string()),
                    Some("host".to_string()),
                    None,
                )
                .unwrap()
        );
        assert_eq!(store.entries.len(), 3);

        Ok(())
    }

    #[test]
    fn test_display_string() {
        let local = BookmarkEntry {
            path: PathBuf::from("/home/user/src"),
            ssh_user: None,
            ssh_host: None,
            ssh_port: None,
        };
        assert_eq!(local.display_string(), "/home/user/src");

        let remote_default = BookmarkEntry {
            path: PathBuf::from("/home/user/src"),
            ssh_user: Some("someuser".to_string()),
            ssh_host: Some("host".to_string()),
            ssh_port: None,
        };
        assert_eq!(
            remote_default.display_string(),
            "someuser@host:/home/user/src"
        );

        let remote_port = BookmarkEntry {
            path: PathBuf::from("/home/user/src"),
            ssh_user: Some("someuser".to_string()),
            ssh_host: Some("host".to_string()),
            ssh_port: Some(2222),
        };
        assert_eq!(
            remote_port.display_string(),
            "someuser@host:2222:/home/user/src"
        );
    }

    #[test]
    fn test_serde_roundtrip_backward_compatible() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("bookmarks.json");
        let mut store = BookmarkStore {
            entries: Vec::new(),
            path: file_path.clone(),
        };

        store.add(PathBuf::from("/home/user/local")).unwrap();
        store
            .add_with_ssh(
                PathBuf::from("/var/remote"),
                Some("bob".to_string()),
                Some("example.com".to_string()),
                Some(22),
            )
            .unwrap();

        let content = serde_json::to_string_pretty(&store.entries)?;
        let reloaded: Vec<BookmarkEntry> = serde_json::from_str(&content)?;
        assert_eq!(reloaded.len(), 2);
        assert!(!reloaded[0].is_remote());
        assert!(reloaded[1].is_remote());
        assert_eq!(reloaded[1].ssh_user.as_deref(), Some("bob"));
        assert_eq!(reloaded[1].ssh_port, Some(22));

        // Local-only legacy entry has None fields
        let legacy = serde_json::from_str::<Vec<BookmarkEntry>>(r#"[{"path":"/legacy/path"}]"#)?;
        assert_eq!(legacy.len(), 1);
        assert!(!legacy[0].is_remote());
        assert_eq!(legacy[0].ssh_user, None);
        assert_eq!(legacy[0].ssh_host, None);
        assert_eq!(legacy[0].ssh_port, None);

        Ok(())
    }

    #[test]
    fn test_fuzzy_search_matches_display() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("bookmarks.json");
        let mut store = BookmarkStore {
            entries: Vec::new(),
            path: file_path,
        };

        store
            .add_with_ssh(
                PathBuf::from("/home/user/src"),
                Some("someuser".to_string()),
                Some("myserver".to_string()),
                None,
            )
            .unwrap();
        let results = store.fuzzy_search("myserver");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], PathBuf::from("/home/user/src"));

        let entries = store.fuzzy_search_entries("myserver");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].display_string(),
            "someuser@myserver:/home/user/src"
        );

        Ok(())
    }
}
