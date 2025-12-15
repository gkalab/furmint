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
}

#[derive(Debug)]
pub enum TaskEvent {
    UpdateStatus(usize, TaskStatus),
    UpdateProgress(usize, usize, usize), // id, processed, total
    Conflict(usize, std::path::PathBuf, ConflictType),
    Error(usize, String, String), // id, path, error_message
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
            name: name.clone(),
            status: TaskStatus::Running,
            progress: None,
            cancel_flag: cancel_flag.clone(),
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
        // We need to match the sort order of get_tasks: by ID
        let mut ids: Vec<usize> = tasks.keys().cloned().collect();
        ids.sort();
        if current < ids.len() {
            Some(ids[current])
        } else {
            None
        }
    }

    // For UI
    pub fn get_tasks(&self) -> Vec<(usize, String, TaskStatus, Option<(usize, usize)>)> {
        let tasks = self.tasks.lock().unwrap();
        let mut result: Vec<_> = tasks
            .iter()
            .map(|(id, t)| (*id, t.name.clone(), t.status.clone(), t.progress))
            .collect();
        result.sort_by_key(|k| k.0);
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
        }
        // Auto-remove completed, failed, or cancelled tasks
        match status {
            TaskStatus::Completed | TaskStatus::Failed(_) | TaskStatus::Cancelled => {
                tasks.remove(&id);
                // Clamp selection
                let len = tasks.len();
                let current = self.selected_index.load(Ordering::Relaxed);
                if len > 0 && current >= len {
                    self.selected_index.store(len - 1, Ordering::Relaxed);
                } else if len == 0 {
                    self.selected_index.store(0, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    pub fn update_task_progress(&self, id: usize, processed: usize, total: usize) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&id) {
            task.progress = Some((processed, total));
        }
    }
}
