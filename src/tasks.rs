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
    pub progress: Option<(usize, usize)>,  // (processed, total)
    pub byte_progress: Option<(u64, u64)>, // (processed_bytes, total_bytes)
    pub rsync: bool,
    pub current_file: Option<String>,
    pub cancel_flag: Arc<AtomicBool>,
    pub completed_at: Option<std::time::Instant>,
}

#[derive(Clone, Debug)]
pub struct TaskInfo {
    pub id: usize,
    pub name: String,
    pub status: TaskStatus,
    pub progress: Option<(usize, usize)>,
    pub byte_progress: Option<(u64, u64)>,
    pub rsync: bool,
    pub current_file: Option<String>,
    pub completed_at: Option<std::time::Instant>,
}

#[derive(Debug)]
pub enum TaskEvent {
    UpdateStatus(usize, TaskStatus),
    UpdateProgress(usize, usize, usize), // id, processed, total
    UpdateByteProgress(usize, u64, u64), // id, processed_bytes, total_bytes
    UpdateCurrentFile(usize, String),    // id, filename
    SetRsyncMode(usize, bool),           // id, is_rsync
    Conflict(usize, std::path::PathBuf, ConflictType),
    Error(usize, String, String), // id, path, error_message
    SshConnected(SshContext),
    SshReconnected(SshContext),
    SshReconnectFailed(String, String), // session_id, error_message
    SshError(String, String, crate::ssh_manager::SshError), // host, user, error
    /// Directory size calculation completed: (`task_id`, path, `size_in_bytes`)
    DirSizeCalculated(usize, std::path::PathBuf, u64),
    ArchiveLoaded(usize, ProviderWrapper, String, std::path::PathBuf), // side_index, provider, filename, path
}

#[derive(Clone)]
pub struct ProviderWrapper(pub Arc<dyn crate::fs::fs_provider::FileSystemProvider>);

impl std::fmt::Debug for ProviderWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FileSystemProvider")
    }
}

#[derive(Clone)]
pub struct SshContext {
    pub provider: Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
    pub path: Option<std::path::PathBuf>,
    pub name: Option<String>,
}

impl std::fmt::Debug for SshContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshContext")
            .field("path", &self.path)
            .field("name", &self.name)
            .field("provider", &"FileSystemProvider")
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
    #[must_use]
    pub fn new(event_tx: mpsc::UnboundedSender<TaskEvent>) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicUsize::new(1),
            event_tx,
            selected_index: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Spawns a new task.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn spawn_task<F, Fut>(&self, name: &str, f: F) -> usize
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
            byte_progress: None,
            rsync: false,
            current_file: None,
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

    /// Cancels a task by ID.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn cancel_task(&self, id: usize) {
        let tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get(&id) {
            task.cancel_flag.store(true, Ordering::Relaxed);
        }
    }

    /// Cancels all running tasks.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn cancel_all_tasks(&self) {
        let tasks = self.tasks.lock().unwrap();
        for task in tasks.values() {
            task.cancel_flag.store(true, Ordering::Relaxed);
        }
    }

    /// Removes all finished tasks.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn remove_finished_tasks(&self) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.retain(|_, task| matches!(task.status, TaskStatus::Running));
        // Reset selection if list changes?
        self.selected_index.store(0, Ordering::Relaxed);
    }

    /// Moves the selection up.
    pub fn move_selection_up(&self) {
        let current = self.selected_index.load(Ordering::Relaxed);
        if current > 0 {
            self.selected_index.store(current - 1, Ordering::Relaxed);
        }
    }

    /// Moves the selection down.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
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

    /// Gets the selected task ID.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
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

    /// Gets all tasks.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn get_tasks(&self) -> Vec<TaskInfo> {
        let tasks = self.tasks.lock().unwrap();
        let mut result: Vec<_> = tasks
            .iter()
            .map(|(id, t)| TaskInfo {
                id: *id,
                name: t.name.clone(),
                status: t.status.clone(),
                progress: t.progress,
                byte_progress: t.byte_progress,
                rsync: t.rsync,
                current_file: t.current_file.clone(),
                completed_at: t.completed_at,
            })
            .collect();
        result.sort_by(|a, b| b.id.cmp(&a.id));
        result
    }

    /// Checks if there are running tasks.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn has_running_tasks(&self) -> bool {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .values()
            .any(|t| matches!(t.status, TaskStatus::Running))
    }

    /// Updates a task's status.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn update_task_status(&self, id: usize, status: &TaskStatus) {
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

    /// Cleans up old completed tasks.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
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

    /// Updates a task's progress.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn update_task_progress(&self, id: usize, processed: usize, total: usize) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&id) {
            task.progress = Some((processed, total));
        }
    }

    /// Updates a task's byte progress.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn update_task_byte_progress(&self, id: usize, processed: u64, total: u64) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&id) {
            task.byte_progress = Some((processed, total));
        }
    }

    /// Updates the current file being processed by a task.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn update_task_current_file(&self, id: usize, filename: String) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&id) {
            task.current_file = Some(filename);
        }
    }

    /// Sets whether a task is in rsync mode.
    ///
    /// # Panics
    ///
    /// Panics if the tasks mutex cannot be locked.
    pub fn update_task_rsync_mode(&self, id: usize, rsync: bool) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&id) {
            task.rsync = rsync;
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

        let id = manager.spawn_task("test", |_cancel, _tx, _id| async move {});

        // Task should be running
        assert_eq!(manager.get_tasks().len(), 1);
        assert_eq!(manager.get_tasks()[0].status, TaskStatus::Running);

        // Mark as completed
        manager.update_task_status(id, &TaskStatus::Completed);

        // Should NOT be removed immediately
        assert_eq!(manager.get_tasks().len(), 1);
        assert_eq!(manager.get_tasks()[0].status, TaskStatus::Completed);
        assert!(manager.get_tasks()[0].completed_at.is_some());

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

        let id1 = tm.spawn_task("task 1", |_cancel, _tx, _id| async move {});
        let id2 = tm.spawn_task("task 2", |_cancel, _tx, _id| async move {});
        let id3 = tm.spawn_task("task 3", |_cancel, _tx, _id| async move {});

        let tasks = tm.get_tasks();
        assert_eq!(tasks.len(), 3);
        // Should be id3, id2, id1
        assert_eq!(tasks[0].id, id3);
        assert_eq!(tasks[1].id, id2);
        assert_eq!(tasks[2].id, id1);

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
        tm.spawn_task("task1", |_c, _tx, _id| async {});
        tm.spawn_task("task2", |_c, _tx, _id| async {});

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
        let id = tm.spawn_task("task1", |_c, _tx, _id| async {});

        tm.update_task_status(id, &TaskStatus::Completed);
        let tasks = tm.get_tasks();
        assert!(matches!(tasks[0].status, TaskStatus::Completed));
        assert!(tasks[0].completed_at.is_some()); // completed_at should be set

        tm.update_task_progress(id, 50, 100);
        let tasks = tm.get_tasks();
        assert_eq!(tasks[0].progress, Some((50, 100)));
    }

    #[tokio::test]
    async fn test_cancel_task() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tm = TaskManager::new(tx);
        let id = tm.spawn_task("task1", |cancel, _tx, _id| async move {
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
