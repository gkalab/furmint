use crate::fs::provider::FileSystemProvider;
use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
use crate::fs::local::LocalFs;

use super::{PersistentPanel, PersistentTab, Tab};

pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active_tab_index: usize,
}

impl TabManager {
    /// Creates a new `TabManager` with a single tab at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the initial directory cannot be read.
    pub async fn new(initial_path: &Path) -> anyhow::Result<Self> {
        let tab = Tab::new(initial_path).await?;
        Ok(Self {
            tabs: vec![tab],
            active_tab_index: 0,
        })
    }

    /// Creates a `TabManager` from a persistent representation.
    ///
    /// # Errors
    ///
    /// Returns an error if no valid tabs can be restored.
    pub async fn from_persistent(p: PersistentPanel) -> anyhow::Result<Self> {
        let mut tabs = Vec::new();
        for pt in p.tabs {
            match Tab::from_persistent(pt).await {
                Ok(t) => tabs.push(t),
                Err(_) => {
                    // If a path no longer exists, fall back to a CWD tab
                    if let Ok(cwd) = std::env::current_dir()
                        && let Ok(t) = Tab::new(&cwd).await
                    {
                        tabs.push(t);
                    }
                }
            }
        }

        if tabs.is_empty() {
            let cwd = std::env::current_dir()?;
            tabs.push(Tab::new(&cwd).await?);
        }

        let active_tab_index = if p.active_tab_index < tabs.len() {
            p.active_tab_index
        } else {
            0
        };

        Ok(Self {
            tabs,
            active_tab_index,
        })
    }

    #[must_use]
    pub fn to_persistent(&self) -> PersistentPanel {
        let tabs: Vec<PersistentTab> = self
            .tabs
            .iter()
            .filter(|t| t.provider.context_key().is_local())
            .map(Tab::to_persistent)
            .collect();

        // If all tabs were remote, ensure we save at least one local tab (CWD or Home)
        // logic handled by loader if empty?
        // Loader: if p.tabs is empty, it adds CWD new tab.
        // So we can just return the filtered list.

        // However, we need to adjust active_tab_index if it pointed to a remote tab.
        // If we filter, indices shift.
        // Simple strategy: save the index of the first local tab that was active, or 0.
        // Or if the active tab was local, map its index.

        let active_tab_opt = self.tabs.get(self.active_tab_index);
        let active_is_local = active_tab_opt.is_some_and(|t| t.provider.context_key().is_local());

        // Recalculate new active index
        let new_active_index = if active_is_local {
            // Count how many local tabs were before it
            self.tabs
                .iter()
                .take(self.active_tab_index)
                .filter(|t| t.provider.context_key().is_local())
                .count()
        } else {
            // If active was remote, just default to 0 (last active local, or first)
            // Ideally we'd like the nearest local tab, but 0 is safe.
            0
        };

        PersistentPanel {
            tabs,
            active_tab_index: new_active_index,
        }
    }

    #[must_use]
    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab_index]
    }

    /// Inserts a tab immediately after the currently active tab and makes it active.
    pub fn insert_tab_after_active(&mut self, tab: Tab) {
        let new_index = self.active_tab_index + 1;
        self.tabs.insert(new_index, tab);
        self.active_tab_index = new_index;
    }

    /// Creates a new tab at the specified path.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    #[cfg(test)]
    pub async fn new_tab(&mut self, path: &Path, cursor: Option<usize>) -> anyhow::Result<()> {
        self.new_tab_with_provider(path, Arc::new(LocalFs::new()), cursor)
            .await
    }

    /// Creates a new tab with a custom filesystem provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    pub async fn new_tab_with_provider(
        &mut self,
        path: &Path,
        provider: Arc<dyn FileSystemProvider>,
        cursor: Option<usize>,
    ) -> anyhow::Result<()> {
        let mut tab = Tab::with_provider(path, provider).await?;
        // Inherit sort settings from current active tab
        {
            let active = self.active_tab();
            tab.sort = active.sort;
        }
        tab.sort_entries();

        if let Some(pos) = cursor.filter(|&c| c < tab.visible_indices.len()) {
            tab.cursor = pos;
        }

        self.insert_tab_after_active(tab);
        Ok(())
    }

    #[must_use]
    pub fn local_tab_count(&self) -> usize {
        self.tabs
            .iter()
            .filter(|t| t.provider.context_key().is_local())
            .count()
    }

    pub fn close_tab(&mut self, index: usize) -> bool {
        if self.tabs.len() > 1 {
            let active = self.active_tab_index;
            self.tabs.remove(index);
            if index == active {
                self.active_tab_index = active.saturating_sub(1);
            } else if index < active {
                self.active_tab_index = active - 1;
            }
            true
        } else {
            false
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab_index = (self.active_tab_index + 1) % self.tabs.len();
    }

    pub fn prev_tab(&mut self) {
        if self.active_tab_index == 0 {
            self.active_tab_index = self.tabs.len() - 1;
        } else {
            self.active_tab_index -= 1;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}
