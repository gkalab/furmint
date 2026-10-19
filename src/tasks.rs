//! Background task management and the application-level UI event bus.
//!
//! Worker tasks (filesystem operations, SSH connections, archive loads, ...)
//! report back to the single-threaded UI over one event channel. Events are
//! grouped per subsystem; the sole dispatcher is
//! [`crate::handlers::event_dispatch::dispatch_ui_event`], which routes each event
//! to its subsystem handler.
//!
//! Workers publish through [`EventBus`] rather than the raw channel. The bus
//! coalesces the idempotent progress fields keep-latest, so a producer that
//! outruns the UI cannot grow the queue without bound.
//!
//! Destructive filesystem work is serialized: [`TaskManager::spawn_queued_task`]
//! admits copy, move, delete and extract one at a time in FIFO order, while
//! read-only work keeps using [`TaskManager::spawn_task`] and runs immediately.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Clone, Debug, PartialEq)]
pub enum TaskStatus {
    /// Waiting behind another destructive operation in the admission queue.
    Queued,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// Starts a queued task's worker and returns its join handle.
type StartTask = Box<dyn FnOnce() -> JoinHandle<()> + Send>;

/// A destructive task that has not been admitted yet.
struct QueuedTask {
    id: usize,
    start: StartTask,
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
    /// The failure text is already on screen (e.g. a panel status line), so
    /// the task result line must not repeat it. The message stays on the task
    /// itself for the task manager list.
    pub error_reported: bool,
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
    pub error_reported: bool,
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
    #[cfg_attr(not(unix), allow(dead_code))]
    SetRsyncMode {
        task_id: usize,
        rsync: bool,
    },
}

impl TaskEvent {
    /// The `(task, field)` slot this event updates, if it is a coalescible
    /// progress snapshot.
    ///
    /// `UpdateStatus` has no slot: the UI has to observe every status change
    /// exactly as sent, so it is never coalesced.
    #[must_use]
    pub fn snapshot_slot(&self) -> Option<(usize, TaskField)> {
        match self {
            Self::UpdateProgress { task_id, .. } => Some((*task_id, TaskField::Progress)),
            Self::UpdateByteProgress { task_id, .. } => Some((*task_id, TaskField::ByteProgress)),
            Self::UpdateCurrentFile { task_id, .. } => Some((*task_id, TaskField::CurrentFile)),
            Self::SetRsyncMode { task_id, .. } => Some((*task_id, TaskField::Rsync)),
            Self::UpdateStatus { .. } => None,
        }
    }
}

/// A task field that is reported as an idempotent snapshot.
///
/// An event for one of these fields means "set field `F` of task `T` to `V`",
/// so an intermediate value is worthless the moment a newer one is sent.
/// [`EventBus`] therefore keeps only the newest value per field per task
/// instead of queueing every step of a burst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskField {
    /// Item counts (`TaskEvent::UpdateProgress`).
    Progress,
    /// Byte counts (`TaskEvent::UpdateByteProgress`).
    ByteProgress,
    /// The file currently being processed (`TaskEvent::UpdateCurrentFile`).
    CurrentFile,
    /// Whether rsync's own progress line is shown (`TaskEvent::SetRsyncMode`).
    Rsync,
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
        error: crate::ssh::manager::SshError,
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
    DirSizeCalculated { path: std::path::PathBuf, size: u64 },
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
    ArchiveOpenFailed {
        task_id: usize,
        side_index: usize,
        path: std::path::PathBuf,
        message: String,
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
/// the sole dispatcher is [`crate::handlers::event_dispatch::dispatch_ui_event`].
#[derive(Debug)]
pub enum UiEvent {
    Task(TaskEvent),
    Ssh(SshEvent),
    Fs(FsEvent),
    Alert(AlertEvent),
    /// A coalesced progress notification. The value it stands for lives in the
    /// [`EventBus`] slot named here and must be fetched with
    /// [`EventBus::take_coalesced`] before it can be applied. At most one of
    /// these is ever queued per `(task_id, field)`.
    TaskSnapshot {
        task_id: usize,
        field: TaskField,
    },
}

/// Publisher handle for the app-level UI event bus.
///
/// Background tasks report through this instead of the raw channel so that
/// progress reporting cannot grow the queue without bound. The four
/// idempotent task fields ([`TaskField`]) are coalesced keep-latest, so a
/// producer that outruns the UI — a recursive copy emitting one
/// `UpdateCurrentFile` per file, an rsync or SFTP byte stream — costs at most
/// one queued event per field per task instead of one per update. Everything
/// else (status changes, SSH prompts, filesystem results, conflict/error
/// alerts) is queued verbatim, because the UI must act on each of them.
///
/// `send` never blocks and never fails: once the consumer is gone, events are
/// dropped exactly as an unbounded channel drops them.
#[derive(Clone, Debug)]
pub struct EventBus {
    tx: mpsc::UnboundedSender<UiEvent>,
    /// Newest not-yet-taken value per coalescible `(task, field)`. A key is
    /// present exactly while its [`UiEvent::TaskSnapshot`] notification is
    /// queued, so this map needs no cleanup of its own: taking the value (or
    /// dropping the bus) is the only way an entry goes away.
    pending: Arc<Mutex<HashMap<(usize, TaskField), TaskEvent>>>,
}

impl EventBus {
    /// Wraps the channel shared with the UI event loop.
    #[must_use]
    pub fn new(tx: mpsc::UnboundedSender<UiEvent>) -> Self {
        Self {
            tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Queues `event` for the UI thread.
    ///
    /// A coalescible progress event stores its value in the `(task, field)`
    /// slot and only queues a [`UiEvent::TaskSnapshot`] when no notification
    /// for that slot is outstanding yet. The queued notification therefore
    /// always picks up the newest value, and the queue stays bounded by the
    /// number of live tasks no matter how many updates arrive.
    pub fn send(&self, event: UiEvent) {
        let UiEvent::Task(task_event) = event else {
            let _ = self.tx.send(event);
            return;
        };
        let Some(slot) = task_event.snapshot_slot() else {
            let _ = self.tx.send(UiEvent::Task(task_event));
            return;
        };

        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.insert(slot, task_event).is_some() {
            // A notification for this slot is already queued and will pick up
            // the value stored above.
            return;
        }
        drop(pending);
        let _ = self.tx.send(UiEvent::TaskSnapshot {
            task_id: slot.0,
            field: slot.1,
        });
    }

    /// Returns true once the consumer is gone and events are being discarded.
    #[cfg(test)]
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// Takes the value behind a [`UiEvent::TaskSnapshot`], if still pending.
    ///
    /// Removing the value is what lets the next update for the same field
    /// queue a fresh notification, so the dispatcher calls this *before*
    /// applying the value.
    #[must_use]
    pub fn take_coalesced(&self, task_id: usize, field: TaskField) -> Option<TaskEvent> {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&(task_id, field))
    }
}

#[derive(Clone)]
pub struct ProviderWrapper(pub Arc<dyn crate::fs::provider::FileSystemProvider>);

impl std::fmt::Debug for ProviderWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FileSystemProvider")
    }
}

#[derive(Clone)]
pub struct SshContext {
    pub provider: Arc<dyn crate::fs::provider::FileSystemProvider>,
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
    event_tx: EventBus,
    selected_index: usize,
    /// Decision senders for copy/move tasks, keyed by task ID.
    pub task_decision_txs: HashMap<usize, mpsc::Sender<TaskDecision>>,
    /// Destructive tasks (copy, move, delete, extract) waiting in FIFO order.
    /// Read-only work never enters this queue.
    queued: VecDeque<QueuedTask>,
    /// The one destructive task currently admitted, if any.
    exclusive_running: Option<usize>,
}

impl TaskManager {
    #[must_use]
    pub fn new(event_tx: mpsc::UnboundedSender<UiEvent>) -> Self {
        Self {
            tasks: BTreeMap::new(),
            next_id: 1,
            event_tx: EventBus::new(event_tx),
            selected_index: 0,
            task_decision_txs: HashMap::new(),
            queued: VecDeque::new(),
            exclusive_running: None,
        }
    }

    #[must_use]
    pub fn get_tx(&self) -> EventBus {
        self.event_tx.clone()
    }

    /// Takes the value behind a pending [`UiEvent::TaskSnapshot`] notification,
    /// as the dispatcher does before applying it.
    #[must_use]
    pub fn take_coalesced(&self, task_id: usize, field: TaskField) -> Option<TaskEvent> {
        self.event_tx.take_coalesced(task_id, field)
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
    ///
    /// This runs immediately and is meant for read-only work (size walks,
    /// listings, connections). Destructive work must use
    /// [`Self::spawn_queued_task`] so it cannot race another destructive task.
    pub fn spawn_task<F, Fut>(&mut self, name: &str, f: F) -> usize
    where
        F: FnOnce(Arc<AtomicBool>, EventBus, usize) -> Fut + Send + 'static,
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
                error_reported: false,
            },
        );
        id
    }

    /// Spawns a destructive task (copy, move, delete, extract) behind the
    /// exclusive-operation queue and returns its ID.
    ///
    /// Only one destructive task runs at a time. A second copy, a paste while
    /// a move is in progress, or a delete behind either is admitted in FIFO
    /// order once the running task finishes, instead of racing it for the same
    /// destination. Read-only work keeps using [`Self::spawn_task`] and is
    /// never delayed.
    pub fn spawn_queued_task<F, Fut>(&mut self, name: &str, f: F) -> usize
    where
        F: FnOnce(Arc<AtomicBool>, EventBus, usize) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id;
        self.next_id += 1;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let worker_flag = Arc::clone(&cancel_flag);
        let tx = self.event_tx.clone();
        let start: StartTask = Box::new(move || {
            tokio::spawn(async move {
                f(worker_flag, tx, id).await;
            })
        });

        self.tasks.insert(
            id,
            Task {
                name: name.to_string(),
                status: TaskStatus::Queued,
                progress: None,
                byte_progress: None,
                rsync: false,
                current_file: None,
                cancel_flag,
                handle: None,
                completed_at: None,
                error_reported: false,
            },
        );
        self.queued.push_back(QueuedTask { id, start });
        self.admit_next_queued();
        id
    }

    /// Admits the oldest queued destructive task when the slot is free.
    ///
    /// Tasks cancelled while waiting are skipped: their status is already
    /// `Cancelled` and their worker must never start.
    fn admit_next_queued(&mut self) {
        if self.exclusive_running.is_some() {
            return;
        }
        while let Some(queued) = self.queued.pop_front() {
            if !matches!(
                self.tasks.get(&queued.id).map(|task| &task.status),
                Some(TaskStatus::Queued)
            ) {
                continue;
            }
            let handle = (queued.start)();
            if let Some(task) = self.tasks.get_mut(&queued.id) {
                task.status = TaskStatus::Running;
                task.handle = Some(handle);
            }
            self.exclusive_running = Some(queued.id);
            return;
        }
    }

    /// Releases the exclusive slot if `id` owned it and admits the next task.
    fn finish_exclusive(&mut self, id: usize) {
        if self.exclusive_running == Some(id) {
            self.exclusive_running = None;
            self.admit_next_queued();
        }
    }

    /// Cancels a task by ID: sets the cooperative flag and aborts the worker
    /// so a stuck task is always terminated. A queued task is removed from the
    /// queue before its worker ever starts.
    pub fn cancel_task(&mut self, id: usize) {
        match self.tasks.get(&id).map(|task| task.status.clone()) {
            Some(TaskStatus::Running) => self.abort_task(id),
            Some(TaskStatus::Queued) => {
                self.queued.retain(|queued| queued.id != id);
                self.task_decision_txs.remove(&id);
                if let Some(task) = self.tasks.get_mut(&id) {
                    task.status = TaskStatus::Cancelled;
                    task.completed_at = Some(std::time::Instant::now());
                }
            }
            _ => {}
        }
    }

    /// Cancels all running and queued tasks.
    pub fn cancel_all_tasks(&mut self) {
        // Drop waiting work first: aborting the admitted task releases the
        // slot, and nothing should be admitted on the way out.
        self.queued.clear();
        let active: Vec<usize> = self
            .tasks
            .iter()
            .filter(|(_, task)| matches!(task.status, TaskStatus::Running | TaskStatus::Queued))
            .map(|(id, _)| *id)
            .collect();
        for id in active {
            self.abort_task(id);
        }
    }

    fn abort_task(&mut self, id: usize) {
        let Some(task) = self.tasks.get_mut(&id) else {
            return;
        };
        task.cancel_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = task.handle.take() {
            handle.abort();
        }
        task.status = TaskStatus::Cancelled;
        task.completed_at = Some(std::time::Instant::now());
        self.finish_exclusive(id);
        self.task_decision_txs.remove(&id);
    }

    /// Removes all finished tasks, keeping the selection on the same task
    /// (or the nearest surviving one). Queued tasks survive because they have
    /// not finished yet.
    pub fn remove_finished_tasks(&mut self) {
        let selected_id = self.ordered_ids().get(self.selected_index).copied();
        self.tasks
            .retain(|_, task| matches!(task.status, TaskStatus::Running | TaskStatus::Queued));
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
                error_reported: t.error_reported,
            })
            .collect()
    }

    /// Checks if there are running or queued tasks.
    #[must_use]
    pub fn has_running_tasks(&self) -> bool {
        self.tasks
            .values()
            .any(|t| matches!(t.status, TaskStatus::Running | TaskStatus::Queued))
    }

    /// Updates a task's status.
    ///
    /// A terminal status on the admitted destructive task releases the slot
    /// and starts the next queued one immediately.
    pub fn update_task_status(&mut self, id: usize, status: &TaskStatus) {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.status = status.clone();
            match status {
                TaskStatus::Completed | TaskStatus::Failed(_) | TaskStatus::Cancelled => {
                    task.completed_at = Some(std::time::Instant::now());
                    self.task_decision_txs.remove(&id);
                }
                TaskStatus::Queued | TaskStatus::Running => {
                    task.completed_at = None;
                }
            }
        }
        if matches!(
            status,
            TaskStatus::Completed | TaskStatus::Failed(_) | TaskStatus::Cancelled
        ) {
            self.finish_exclusive(id);
        }
    }

    pub fn mark_error_reported(&mut self, id: usize) {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.error_reported = true;
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
mod tests;
