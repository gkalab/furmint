//! Background task management and the application-level UI event bus.
//!
//! Worker tasks (filesystem operations, SSH connections, archive loads, ...)
//! report back to the single-threaded UI over one event channel. Events are
//! grouped per subsystem; the sole dispatcher is
//! [`crate::handlers::popup_misc::dispatch_ui_event`], which routes each event
//! to its subsystem handler.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

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
    pub handle: Option<JoinHandle<()>>,
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

/// Task lifecycle and progress updates.
#[derive(Debug)]
pub enum TaskEvent {
    UpdateStatus {
        task_id: usize,
        status: TaskStatus,
    },
    UpdateProgress {
        task_id: usize,
        processed: usize,
        total: usize,
    },
    UpdateByteProgress {
        task_id: usize,
        processed: u64,
        total: u64,
    },
    UpdateCurrentFile {
        task_id: usize,
        filename: String,
    },
    SetRsyncMode {
        task_id: usize,
        rsync: bool,
    },
}

/// SSH connection lifecycle: connect, reconnect, errors, host-key prompts.
#[derive(Debug)]
pub enum SshEvent {
    Connected(SshContext),
    Reconnected(SshContext),
    ReconnectFailed {
        session_id: String,
        error: String,
    },
    Error {
        host: String,
        user: String,
        error: crate::ssh_manager::SshError,
    },
    HostKey {
        host: String,
        port: u16,
        user: String,
        presented_fp: String,
        stored_fp: Option<String>,
        key_line: String,
        password: Option<secrecy::SecretString>,
        target_path: Option<String>,
        key_auth: bool,
        connection_name: Option<String>,
    },
}

/// Filesystem results: directory sizes, remote reloads, archive loads.
#[derive(Debug)]
pub enum FsEvent {
    /// Directory size calculation completed.
    DirSizeCalculated {
        task_id: usize,
        path: std::path::PathBuf,
        size: u64,
    },
    RemoteReloadCompleted {
        side: crate::app_state::tabs::PanelSide,
        tab_index: usize,
        current_dir: std::path::PathBuf,
        result: Result<Vec<crate::fs::utils::FileEntry>, String>,
    },
    ArchiveLoaded {
        side_index: usize,
        provider: ProviderWrapper,
        filename: String,
        path: std::path::PathBuf,
    },
}

/// User-facing alerts raised by background tasks (conflict and error popups).
#[derive(Debug)]
pub enum AlertEvent {
    Conflict {
        task_id: usize,
        path: std::path::PathBuf,
        conflict_type: ConflictType,
    },
    TaskError {
        task_id: usize,
        path: String,
        message: String,
    },
}

/// App-level event bus: worker-to-UI messages from all subsystems.
///
/// Events flow from background tasks to the UI thread over a single channel;
/// the sole dispatcher is [`crate::handlers::popup_misc::dispatch_ui_event`].
#[derive(Debug)]
pub enum UiEvent {
    Task(TaskEvent),
    Ssh(SshEvent),
    Fs(FsEvent),
    Alert(AlertEvent),
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
    pub session_id: Option<String>,
}

impl std::fmt::Debug for SshContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshContext")
            .field("path", &self.path)
            .field("name", &self.name)
            .field("session_id", &self.session_id)
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

/// Tracks background tasks and their progress.
///
/// All access happens on the UI thread — workers only send events over the
/// bus — so no interior mutability is needed.
pub struct TaskManager {
    tasks: BTreeMap<usize, Task>,
    next_id: usize,
    event_tx: mpsc::UnboundedSender<UiEvent>,
    selected_index: usize,
    /// Decision senders for copy/move tasks, keyed by task ID.
    pub task_decision_txs: HashMap<usize, mpsc::Sender<TaskDecision>>,
}

impl TaskManager {
    #[must_use]
    pub fn new(event_tx: mpsc::UnboundedSender<UiEvent>) -> Self {
        Self {
            tasks: BTreeMap::new(),
            next_id: 1,
            event_tx,
            selected_index: 0,
            task_decision_txs: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get_tx(&self) -> mpsc::UnboundedSender<UiEvent> {
        self.event_tx.clone()
    }

    /// Selected index in display order (newest first).
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Task IDs in display order (newest first).
    fn ordered_ids(&self) -> Vec<usize> {
        self.tasks.keys().rev().copied().collect()
    }

    /// Spawns a new task and returns its ID. The worker reports back over the
    /// event bus.
    pub fn spawn_task<F, Fut>(&mut self, name: &str, f: F) -> usize
    where
        F: FnOnce(Arc<AtomicBool>, mpsc::UnboundedSender<UiEvent>, usize) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id;
        self.next_id += 1;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let worker_flag = Arc::clone(&cancel_flag);
        let tx = self.event_tx.clone();
        let handle = tokio::spawn(async move {
            f(worker_flag, tx, id).await;
        });

        self.tasks.insert(
            id,
            Task {
                name: name.to_string(),
                status: TaskStatus::Running,
                progress: None,
                byte_progress: None,
                rsync: false,
                current_file: None,
                cancel_flag,
                handle: Some(handle),
                completed_at: None,
            },
        );
        id
    }

    /// Cancels a task by ID: sets the cooperative flag and aborts the worker
    /// so a stuck task is always terminated.
    pub fn cancel_task(&mut self, id: usize) {
        if let Some(task) = self.tasks.get_mut(&id)
            && matches!(task.status, TaskStatus::Running)
        {
            Self::abort_task(task);
        }
    }

    /// Cancels all running tasks.
    pub fn cancel_all_tasks(&mut self) {
        let running: Vec<usize> = self
            .tasks
            .iter()
            .filter(|(_, task)| matches!(task.status, TaskStatus::Running))
            .map(|(id, _)| *id)
            .collect();
        for id in running {
            if let Some(task) = self.tasks.get_mut(&id) {
                Self::abort_task(task);
            }
        }
    }

    fn abort_task(task: &mut Task) {
        task.cancel_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = task.handle.take() {
            handle.abort();
            task.status = TaskStatus::Cancelled;
            task.completed_at = Some(std::time::Instant::now());
        }
    }

    /// Removes all finished tasks, keeping the selection on the same task
    /// (or the nearest surviving one).
    pub fn remove_finished_tasks(&mut self) {
        let selected_id = self.ordered_ids().get(self.selected_index).copied();
        self.tasks
            .retain(|_, task| matches!(task.status, TaskStatus::Running));
        let len = self.tasks.len();
        self.selected_index = if len == 0 {
            0
        } else {
            let pos = selected_id
                .and_then(|id| self.ordered_ids().iter().position(|i| *i == id))
                .unwrap_or(self.selected_index);
            pos.min(len - 1)
        };
    }

    /// Moves the selection up.
    pub fn move_selection_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    /// Moves the selection down.
    pub fn move_selection_down(&mut self) {
        if self.selected_index + 1 < self.tasks.len() {
            self.selected_index += 1;
        }
    }

    /// Gets the selected task ID.
    #[must_use]
    pub fn get_selected_task_id(&self) -> Option<usize> {
        self.ordered_ids().get(self.selected_index).copied()
    }

    /// Gets all tasks in display order (newest first).
    #[must_use]
    pub fn get_tasks(&self) -> Vec<TaskInfo> {
        self.tasks
            .iter()
            .rev()
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
            .collect()
    }

    /// Checks if there are running tasks.
    #[must_use]
    pub fn has_running_tasks(&self) -> bool {
        self.tasks
            .values()
            .any(|t| matches!(t.status, TaskStatus::Running))
    }

    /// Updates a task's status.
    pub fn update_task_status(&mut self, id: usize, status: &TaskStatus) {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.status = status.clone();
            match status {
                TaskStatus::Completed | TaskStatus::Failed(_) | TaskStatus::Cancelled => {
                    task.completed_at = Some(std::time::Instant::now());
                    self.task_decision_txs.remove(&id);
                }
                TaskStatus::Running => {
                    task.completed_at = None;
                }
            }
        }
    }

    /// Cleans up completed tasks older than 10 seconds.
    pub fn cleanup_tasks(&mut self) {
        let now = std::time::Instant::now();
        self.tasks.retain(|_, task| {
            task.completed_at.is_none_or(|completed_at| {
                now.duration_since(completed_at) < std::time::Duration::from_secs(10)
            })
        });
        self.task_decision_txs
            .retain(|id, _| self.tasks.contains_key(id));

        let len = self.tasks.len();
        if len == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= len {
            self.selected_index = len - 1;
        }
    }

    /// Updates a task's progress.
    pub fn update_task_progress(&mut self, id: usize, processed: usize, total: usize) {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.progress = Some((processed, total));
        }
    }

    /// Updates a task's byte progress.
    pub fn update_task_byte_progress(&mut self, id: usize, processed: u64, total: u64) {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.byte_progress = Some((processed, total));
        }
    }

    /// Updates the current file being processed by a task.
    pub fn update_task_current_file(&mut self, id: usize, filename: String) {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.current_file = Some(filename);
        }
    }

    /// Sets whether a task is in rsync mode.
    pub fn update_task_rsync_mode(&mut self, id: usize, rsync: bool) {
        if let Some(task) = self.tasks.get_mut(&id) {
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
        let mut manager = TaskManager::new(tx);

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
    }

    #[tokio::test]
    async fn test_task_order_descending() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut tm = TaskManager::new(tx);

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
        tm.selected_index = 0;
        assert_eq!(tm.get_selected_task_id(), Some(id3));
        tm.selected_index = 2;
        assert_eq!(tm.get_selected_task_id(), Some(id1));
    }

    #[tokio::test]
    async fn test_task_selection_navigation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tm = TaskManager::new(tx);
        tm.spawn_task("task1", |_c, _tx, _id| async {});
        tm.spawn_task("task2", |_c, _tx, _id| async {});

        assert_eq!(tm.selected_index(), 0);
        tm.move_selection_down();
        assert_eq!(tm.selected_index(), 1);
        tm.move_selection_down(); // should clamp
        assert_eq!(tm.selected_index(), 1);
        tm.move_selection_up();
        assert_eq!(tm.selected_index(), 0);
        tm.move_selection_up(); // should clamp
        assert_eq!(tm.selected_index(), 0);
    }

    #[tokio::test]
    async fn test_task_status_updates() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tm = TaskManager::new(tx);
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
    async fn test_cancel_task_aborts_worker() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tm = TaskManager::new(tx);
        let id = tm.spawn_task("task1", |_c, _tx, _id| async {
            // Never completes on its own — only abort can end this.
            std::future::pending::<()>().await;
        });

        assert!(tm.has_running_tasks());
        tm.cancel_task(id);
        let task = &tm.tasks[&id];
        assert!(task.cancel_flag.load(Ordering::Relaxed));
        assert!(
            task.handle.is_none(),
            "worker handle should have been aborted"
        );
        assert_eq!(task.status, TaskStatus::Cancelled);
        assert!(!tm.has_running_tasks());
    }

    #[tokio::test]
    async fn test_remove_finished_preserves_selection() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tm = TaskManager::new(tx);
        let id1 = tm.spawn_task("task1", |_c, _tx, _id| async {});
        let id2 = tm.spawn_task("task2", |_c, _tx, _id| async {});
        let id3 = tm.spawn_task("task3", |_c, _tx, _id| async {});

        // Display order: id3 (0), id2 (1), id1 (2)
        tm.selected_index = 1; // select id2
        tm.update_task_status(id3, &TaskStatus::Completed);
        tm.remove_finished_tasks();

        assert_eq!(tm.ordered_ids(), vec![id2, id1]);
        assert_eq!(
            tm.get_selected_task_id(),
            Some(id2),
            "selection should follow its task"
        );

        // Remove the selected task itself: selection should fall back to the
        // nearest surviving task instead of resetting.
        tm.update_task_status(id2, &TaskStatus::Failed("boom".to_string()));
        tm.remove_finished_tasks();
        assert_eq!(tm.ordered_ids(), vec![id1]);
        assert_eq!(tm.get_selected_task_id(), Some(id1));
        assert_eq!(tm.selected_index(), 0);
    }
}
