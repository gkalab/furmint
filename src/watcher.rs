use notify::{Config, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, FileIdMap, new_debouncer_opt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
pub enum WatcherEvent {
    FileSystemChange(Vec<PathBuf>),
    Error(String),
    RemoteReloadRequested,
}

pub trait FileSystemWatcher {
    fn update_watched_paths(&mut self, paths: &[PathBuf]) -> anyhow::Result<()>;
    fn poll(&mut self) -> anyhow::Result<()>;
    fn watch(&mut self, path: &Path) -> anyhow::Result<()>;
    fn unwatch(&mut self, path: &Path) -> anyhow::Result<()>;
    fn watched_paths(&self) -> Vec<PathBuf>;
}

pub struct AppWatcher {
    debouncer: Debouncer<RecommendedWatcher, FileIdMap>,
    pub watched_paths: Vec<PathBuf>,
}

impl AppWatcher {
    pub fn new(tx: UnboundedSender<WatcherEvent>) -> anyhow::Result<Self> {
        // Create a debouncer that sends events to the channel
        // We need to clone tx because the closure can differ
        let tx = tx.clone();

        let debouncer = new_debouncer_opt::<_, RecommendedWatcher, FileIdMap>(
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
            FileIdMap::new(),
            Config::default(),
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

impl FileSystemWatcher for AppWatcher {
    fn update_watched_paths(&mut self, paths: &[PathBuf]) -> anyhow::Result<()> {
        self.update_watched_paths(paths)
    }

    fn poll(&mut self) -> anyhow::Result<()> {
        Ok(()) // Local watcher is event-driven
    }

    fn watch(&mut self, path: &Path) -> anyhow::Result<()> {
        self.watch(path)
    }

    fn unwatch(&mut self, path: &Path) -> anyhow::Result<()> {
        self.unwatch(path)
    }

    fn watched_paths(&self) -> Vec<PathBuf> {
        self.watched_paths.clone()
    }
}

pub struct RemoteWatcher {
    tx: UnboundedSender<WatcherEvent>,
    last_poll: std::time::Instant,
}

impl RemoteWatcher {
    pub fn new(tx: UnboundedSender<WatcherEvent>) -> Self {
        Self {
            tx,
            last_poll: std::time::Instant::now(),
        }
    }
}

impl FileSystemWatcher for RemoteWatcher {
    fn update_watched_paths(&mut self, _paths: &[PathBuf]) -> anyhow::Result<()> {
        Ok(()) // Polling logic handles path changes implicitly via active tabs
    }

    fn poll(&mut self) -> anyhow::Result<()> {
        if self.last_poll.elapsed() >= Duration::from_secs(5) {
            let _ = self.tx.send(WatcherEvent::RemoteReloadRequested);
            self.last_poll = std::time::Instant::now();
        }
        Ok(())
    }

    fn watch(&mut self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn unwatch(&mut self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn watched_paths(&self) -> Vec<PathBuf> {
        Vec::new()
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
        watcher
            .update_watched_paths(std::slice::from_ref(&p2))
            .unwrap();
        assert!(!watcher.watched_paths.contains(&p1));
        assert!(watcher.watched_paths.contains(&p2));
    }

    /// This test ensures that the debouncer is indeed using a `FileIdMap`.
    /// If someone changes the type to `NoCache`, this test will fail to compile.
    #[test]
    fn test_debouncer_uses_file_id_map() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let watcher = AppWatcher::new(tx).unwrap();

        // This is a type-level assertion.
        // We try to pass the debouncer to a function that explicitly expects FileIdMap.
        fn assert_file_id_map_cache<W: notify::Watcher>(_d: &Debouncer<W, FileIdMap>) {}

        assert_file_id_map_cache(&watcher.debouncer);
    }
}
