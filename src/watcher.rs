use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
pub enum WatcherEvent {
    FileSystemChange(Vec<PathBuf>),
    Error(String),
}

pub struct AppWatcher {
    debouncer: Debouncer<RecommendedWatcher, NoCache>,
    pub watched_paths: Vec<PathBuf>,
}

impl AppWatcher {
    pub fn new(tx: UnboundedSender<WatcherEvent>) -> anyhow::Result<Self> {
        // Create a debouncer that sends events to the channel
        // We need to clone tx because the closure can differ
        let tx = tx.clone();

        let debouncer = new_debouncer(
            Duration::from_millis(250),
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    let mut paths = Vec::new();
                    for e in events {
                        paths.extend(e.paths.clone());
                    }
                    if !paths.is_empty() {
                        let _ = tx.send(WatcherEvent::FileSystemChange(paths));
                    }
                }
                Err(errors) => {
                    for err in errors {
                        let _ = tx.send(WatcherEvent::Error(err.to_string()));
                    }
                }
            },
        )?;

        Ok(Self {
            debouncer,
            watched_paths: Vec::new(),
        })
    }

    pub fn watch(&mut self, path: &Path) -> anyhow::Result<()> {
        if !self.watched_paths.contains(&path.to_path_buf()) {
            // Watch non-recursive for current directory content
            self.debouncer.watch(path, RecursiveMode::NonRecursive)?;
            self.watched_paths.push(path.to_path_buf());
        }
        Ok(())
    }

    pub fn unwatch(&mut self, path: &Path) -> anyhow::Result<()> {
        if let Some(pos) = self.watched_paths.iter().position(|p| p == path) {
            if let Err(e) = self.debouncer.unwatch(path) {
                // Ignore "No watch was found" error as it might have been removed implicitly
                if !e.to_string().contains("No watch was found") {
                    return Err(anyhow::anyhow!(e));
                }
            }
            self.watched_paths.swap_remove(pos);
        }
        Ok(())
    }

    pub fn update_watched_paths(&mut self, desired_paths: &[PathBuf]) -> anyhow::Result<()> {
        // Remove paths no longer needed
        let current_paths = self.watched_paths.clone();
        for path in &current_paths {
            if !desired_paths.contains(path) {
                self.unwatch(path)?;
            }
        }

        // Add new paths
        for path in desired_paths {
            if !self.watched_paths.contains(path) {
                self.watch(path)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    // Test new() sets up debouncer and empty watched_paths
    #[test]
    fn test_new_initializes_state() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let watcher = AppWatcher::new(tx);
        assert!(watcher.is_ok());
        let watcher = watcher.unwrap();
        assert_eq!(watcher.watched_paths.len(), 0);
    }

    // Test watch() adds a new path
    #[test]
    fn test_watch_adds_new_path() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut watcher = AppWatcher::new(tx).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let res = watcher.watch(&path);
        assert!(res.is_ok());
        assert!(watcher.watched_paths.contains(&path));
    }

    #[test]
    fn test_unwatch_removes_path() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut watcher = AppWatcher::new(tx).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        watcher.watch(&path).unwrap();
        assert!(watcher.watched_paths.contains(&path));
        watcher.unwatch(&path).unwrap();
        assert!(!watcher.watched_paths.contains(&path));
    }

    #[test]
    fn test_update_watched_paths_sync() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut watcher = AppWatcher::new(tx).unwrap();
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let p1 = dir1.path().to_path_buf();
        let p2 = dir2.path().to_path_buf();
        watcher.watch(&p1).unwrap();
        watcher.update_watched_paths(&[p2.clone()]).unwrap();
        assert!(!watcher.watched_paths.contains(&p1));
        assert!(watcher.watched_paths.contains(&p2));
    }
}
