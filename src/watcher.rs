use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{DebounceEventResult, Debouncer, FileIdMap, new_debouncer};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
pub enum WatcherEvent {
    FileSystemChange(Vec<PathBuf>),
    Error(String),
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
            self.debouncer
                .watcher()
                .watch(path, RecursiveMode::NonRecursive)?;
            self.debouncer
                .cache()
                .add_root(path, RecursiveMode::NonRecursive);
            self.watched_paths.push(path.to_path_buf());
        }
        Ok(())
    }

    pub fn unwatch(&mut self, path: &Path) -> anyhow::Result<()> {
        if let Some(pos) = self.watched_paths.iter().position(|p| p == path) {
            if let Err(e) = self.debouncer.watcher().unwatch(path) {
                // Ignore "No watch was found" error as it might have been removed implicitly
                if !e.to_string().contains("No watch was found") {
                    return Err(anyhow::anyhow!(e));
                }
            }
            self.debouncer.cache().remove_root(path);
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
