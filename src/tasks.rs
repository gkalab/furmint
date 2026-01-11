// Clippy allows - complex type is acceptable for task representation
#![allow(clippy::type_complexity)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Clone, Debug, PartialEq)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

pub struct Task {
    pub name: String,
    pub status: TaskStatus,
    pub progress: Option<(usize, usize)>, // (processed, total)
    pub cancel_flag: Arc<AtomicBool>,
    pub completed_at: Option<std::time::Instant>,
}

#[derive(Debug)]
pub enum TaskEvent {
    UpdateStatus(usize, TaskStatus),
    UpdateProgress(usize, usize, usize), // id, processed, total
    Conflict(usize, std::path::PathBuf, ConflictType),
    Error(usize, String, String), // id, path, error_message
    SshConnected(SshContext),
    SshReconnected(SshContext),
    SshReconnectFailed(String, String), // session_id, error_message
    SshAuthFailed(String, String, String), // host, user, error_message
}

#[derive(Clone)]
pub struct SshContext {
    pub provider: Arc<dyn crate::fs_provider::FileSystemProvider>,
    pub path: Option<std::path::PathBuf>,
    pub name: Option<String>,
}

impl std::fmt::Debug for SshContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshContext")
            .field("path", &self.path)
            .field("name", &self.name)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConflictType {
    FileExists,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskDecision {
    Merge,

    Overwrite,
    OverwriteAll,
    Skip,
    SkipAll,
    Retry,

    Cancel,
}

pub struct TaskManager {
    tasks: Arc<Mutex<HashMap<usize, Task>>>,
    next_id: AtomicUsize,
    event_tx: mpsc::UnboundedSender<TaskEvent>,
    pub selected_index: Arc<AtomicUsize>,
}

impl TaskManager {
    pub fn new(event_tx: mpsc::UnboundedSender<TaskEvent>) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicUsize::new(1),
            event_tx,
            selected_index: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn spawn_task<F, Fut>(&self, name: String, f: F) -> usize
    where
        F: FnOnce(Arc<AtomicBool>, mpsc::UnboundedSender<TaskEvent>, usize) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let task = Task {
            name: name.to_string(),
            status: TaskStatus::Running,
            progress: None,
            cancel_flag: cancel_flag.clone(),
            completed_at: None,
        };

        {
            let mut tasks = self.tasks.lock().unwrap();
            tasks.insert(id, task);
        }

        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            f(cancel_flag, tx.clone(), id).await;
        });

        id
    }

    pub fn cancel_task(&self, id: usize) {
        let tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get(&id) {
            task.cancel_flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn remove_finished_tasks(&self) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.retain(|_, task| matches!(task.status, TaskStatus::Running));
        // Reset selection if list changes?
        self.selected_index.store(0, Ordering::Relaxed);
    }

    pub fn move_selection_up(&self) {
        let current = self.selected_index.load(Ordering::Relaxed);
        if current > 0 {
            self.selected_index.store(current - 1, Ordering::Relaxed);
        }
    }

    pub fn move_selection_down(&self) {
        let tasks = self.tasks.lock().unwrap();
        let len = tasks.len();
        if len > 0 {
            let current = self.selected_index.load(Ordering::Relaxed);
            if current + 1 < len {
                self.selected_index.store(current + 1, Ordering::Relaxed);
            }
        }
    }

    pub fn get_selected_task_id(&self) -> Option<usize> {
        let tasks = self.tasks.lock().unwrap();
        if tasks.is_empty() {
            return None;
        }
        let current = self.selected_index.load(Ordering::Relaxed);
        // We need to match the sort order of get_tasks: by ID descending
        let mut ids: Vec<usize> = tasks.keys().copied().collect();
        ids.sort_unstable_by(|a, b| b.cmp(a));
        if current < ids.len() {
            Some(ids[current])
        } else {
            None
        }
    }

    pub fn get_tasks(
        &self,
    ) -> Vec<(
        usize,
        String,
        TaskStatus,
        Option<(usize, usize)>,
        Option<std::time::Instant>,
    )> {
        let tasks = self.tasks.lock().unwrap();
        let mut result: Vec<_> = tasks
            .iter()
            .map(|(id, t)| {
                (
                    *id,
                    t.name.clone(),
                    t.status.clone(),
                    t.progress,
                    t.completed_at,
                )
            })
            .collect();
        result.sort_by(|a, b| b.0.cmp(&a.0));
        result
    }

    pub fn has_running_tasks(&self) -> bool {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .values()
            .any(|t| matches!(t.status, TaskStatus::Running))
    }

    pub fn update_task_status(&self, id: usize, status: TaskStatus) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&id) {
            task.status = status.clone();
            match status {
                TaskStatus::Completed | TaskStatus::Failed(_) | TaskStatus::Cancelled => {
                    task.completed_at = Some(std::time::Instant::now());
                }
                TaskStatus::Running => {
                    task.completed_at = None;
                }
            }
        }
    }

    pub fn cleanup_tasks(&self) {
        let mut tasks = self.tasks.lock().unwrap();
        let now = std::time::Instant::now();
        tasks.retain(|_, task| {
            if let Some(completed_at) = task.completed_at {
                now.duration_since(completed_at) < std::time::Duration::from_secs(10)
            } else {
                true
            }
        });

        // Clamp selection
        let len = tasks.len();
        let current = self.selected_index.load(Ordering::Relaxed);
        if len > 0 && current >= len {
            self.selected_index.store(len - 1, Ordering::Relaxed);
        } else if len == 0 {
            self.selected_index.store(0, Ordering::Relaxed);
        }
    }

    pub fn update_task_progress(&self, id: usize, processed: usize, total: usize) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&id) {
            task.progress = Some((processed, total));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_task_cleanup() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let manager = TaskManager::new(tx);

        let id = manager.spawn_task("test".to_string(), |_cancel, _tx, _id| async move {});

        // Task should be running
        assert_eq!(manager.get_tasks().len(), 1);
        assert_eq!(manager.get_tasks()[0].2, TaskStatus::Running);

        // Mark as completed
        manager.update_task_status(id, TaskStatus::Completed);

        // Should NOT be removed immediately
        assert_eq!(manager.get_tasks().len(), 1);
        assert_eq!(manager.get_tasks()[0].2, TaskStatus::Completed);
        assert!(manager.get_tasks()[0].4.is_some());

        // Cleanup should not remove it yet (it's new)
        manager.cleanup_tasks();
        assert_eq!(manager.get_tasks().len(), 1);

        // Manually manipulate completed_at for testing removal (if we could, but it's private field of Task in HashMap)
        // Since we can't easily manipulate time in Instant without mocks,
        // we've at least verified it's not removed immediately.
    }

    #[tokio::test]
    async fn test_task_order_descending() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let tm = TaskManager::new(tx);

        let id1 = tm.spawn_task("task 1".to_string(), |_cancel, _tx, _id| async move {});
        let id2 = tm.spawn_task("task 2".to_string(), |_cancel, _tx, _id| async move {});
        let id3 = tm.spawn_task("task 3".to_string(), |_cancel, _tx, _id| async move {});

        let tasks = tm.get_tasks();
        assert_eq!(tasks.len(), 3);
        // Should be id3, id2, id1
        assert_eq!(tasks[0].0, id3);
        assert_eq!(tasks[1].0, id2);
        assert_eq!(tasks[2].0, id1);

        // Verify selected task id mapping matches visual order
        tm.selected_index.store(0, Ordering::Relaxed);
        assert_eq!(tm.get_selected_task_id(), Some(id3));
        tm.selected_index.store(2, Ordering::Relaxed);
        assert_eq!(tm.get_selected_task_id(), Some(id1));
    }

    #[tokio::test]
    async fn test_task_selection_navigation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tm = TaskManager::new(tx);
        tm.spawn_task("task1".to_string(), |_c, _tx, _id| async {});
        tm.spawn_task("task2".to_string(), |_c, _tx, _id| async {});

        assert_eq!(tm.selected_index.load(Ordering::Relaxed), 0);
        tm.move_selection_down();
        assert_eq!(tm.selected_index.load(Ordering::Relaxed), 1);
        tm.move_selection_down(); // should clamp
        assert_eq!(tm.selected_index.load(Ordering::Relaxed), 1);
        tm.move_selection_up();
        assert_eq!(tm.selected_index.load(Ordering::Relaxed), 0);
        tm.move_selection_up(); // should clamp
        assert_eq!(tm.selected_index.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_task_status_updates() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tm = TaskManager::new(tx);
        let id = tm.spawn_task("task1".to_string(), |_c, _tx, _id| async {});

        tm.update_task_status(id, TaskStatus::Completed);
        let tasks = tm.get_tasks();
        assert!(matches!(tasks[0].2, TaskStatus::Completed));
        assert!(tasks[0].4.is_some()); // completed_at should be set

        tm.update_task_progress(id, 50, 100);
        let tasks = tm.get_tasks();
        assert_eq!(tasks[0].3, Some((50, 100)));
    }

    #[tokio::test]
    async fn test_cancel_task() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tm = TaskManager::new(tx);
        let id = tm.spawn_task("task1".to_string(), |cancel, _tx, _id| async move {
            while !cancel.load(Ordering::Relaxed) {
                tokio::task::yield_now().await;
            }
        });

        assert!(tm.has_running_tasks());
        tm.cancel_task(id);
        // We might need to wait a bit for the task to react, but cancel_task sets the flag
        let tasks = tm.tasks.lock().unwrap();
        assert!(tasks.get(&id).unwrap().cancel_flag.load(Ordering::Relaxed));
    }
}
