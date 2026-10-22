use super::*;

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
async fn test_queued_tasks_admit_one_at_a_time() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tm = TaskManager::new(tx);

    let copy = tm.spawn_queued_task("copy", |_c, _tx, _id| async {});
    let move_task = tm.spawn_queued_task("move", |_c, _tx, _id| async {});

    assert_eq!(tm.tasks[&copy].status, TaskStatus::Running);
    assert_eq!(tm.tasks[&move_task].status, TaskStatus::Queued);
    assert!(
        tm.tasks[&move_task].handle.is_none(),
        "a queued worker must not be spawned yet"
    );

    tm.update_task_status(copy, &TaskStatus::Completed);

    assert_eq!(tm.tasks[&move_task].status, TaskStatus::Running);
    assert!(tm.tasks[&move_task].handle.is_some());
    assert!(tm.queued.is_empty());
}

#[tokio::test]
async fn test_queued_tasks_are_admitted_fifo() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tm = TaskManager::new(tx);

    let first = tm.spawn_queued_task("first", |_c, _tx, _id| async {});
    let second = tm.spawn_queued_task("second", |_c, _tx, _id| async {});
    let third = tm.spawn_queued_task("third", |_c, _tx, _id| async {});

    tm.update_task_status(first, &TaskStatus::Completed);
    assert_eq!(tm.tasks[&second].status, TaskStatus::Running);
    assert_eq!(tm.tasks[&third].status, TaskStatus::Queued);

    tm.update_task_status(second, &TaskStatus::Failed("boom".to_string()));
    assert_eq!(tm.tasks[&third].status, TaskStatus::Running);
}

#[tokio::test]
async fn test_read_only_tasks_are_not_queued() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tm = TaskManager::new(tx);

    let destructive = tm.spawn_queued_task("copy", |_c, _tx, _id| async {});
    let read_only = tm.spawn_task("calculate size", |_c, _tx, _id| async {});

    assert_eq!(tm.tasks[&destructive].status, TaskStatus::Running);
    assert_eq!(
        tm.tasks[&read_only].status,
        TaskStatus::Running,
        "read-only work must not wait behind destructive work"
    );
}

#[tokio::test]
async fn test_cancelling_a_queued_task_never_starts_it() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tm = TaskManager::new(tx);
    let started = Arc::new(AtomicBool::new(false));

    let first = tm.spawn_queued_task("copy", |_c, _tx, _id| async {});
    let started_in_worker = Arc::clone(&started);
    let second = tm.spawn_queued_task("move", move |_c, _tx, _id| async move {
        started_in_worker.store(true, Ordering::Relaxed);
    });

    tm.cancel_task(second);
    assert_eq!(tm.tasks[&second].status, TaskStatus::Cancelled);

    tm.update_task_status(first, &TaskStatus::Completed);
    tokio::task::yield_now().await;

    assert!(
        !started.load(Ordering::Relaxed),
        "a cancelled queued worker must never run"
    );
    assert_eq!(tm.tasks[&second].status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn test_cancel_all_drops_queued_tasks() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tm = TaskManager::new(tx);

    let first = tm.spawn_queued_task("copy", |_c, _tx, _id| async {
        std::future::pending::<()>().await;
    });
    let second = tm.spawn_queued_task("move", |_c, _tx, _id| async {});

    tm.cancel_all_tasks();

    assert_eq!(tm.tasks[&first].status, TaskStatus::Cancelled);
    assert_eq!(tm.tasks[&second].status, TaskStatus::Cancelled);
    assert!(tm.queued.is_empty());
    assert!(!tm.has_running_tasks());
}

#[tokio::test]
async fn test_remove_finished_preserves_queued_tasks() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tm = TaskManager::new(tx);

    let first = tm.spawn_queued_task("copy", |_c, _tx, _id| async {});
    let second = tm.spawn_queued_task("move", |_c, _tx, _id| async {});

    tm.update_task_status(first, &TaskStatus::Completed);
    tm.remove_finished_tasks();

    assert_eq!(tm.ordered_ids(), vec![second]);
    assert_eq!(tm.tasks[&second].status, TaskStatus::Running);
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
