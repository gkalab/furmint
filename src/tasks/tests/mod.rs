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

fn update_progress(task_id: usize, processed: usize) -> UiEvent {
    UiEvent::Task(TaskEvent::UpdateProgress {
        task_id,
        processed,
        total: 1_000_000,
    })
}

fn current_file(task_id: usize, filename: &str) -> UiEvent {
    UiEvent::Task(TaskEvent::UpdateCurrentFile {
        task_id,
        filename: filename.to_string(),
    })
}

fn drain(rx: &mut mpsc::UnboundedReceiver<UiEvent>) -> Vec<UiEvent> {
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

/// The `(task, field)` slot of every queued event, failing on anything that
/// is not a coalesced notification.
fn snapshot_slots(events: Vec<UiEvent>) -> Vec<(usize, TaskField)> {
    events
        .into_iter()
        .map(|e| match e {
            UiEvent::TaskSnapshot { task_id, field } => (task_id, field),
            other => panic!("expected a snapshot, got {other:?}"),
        })
        .collect()
}

/// The bus must not grow with the number of updates a producer sends: a
/// fast progress reporter against a stalled UI would otherwise queue every
/// intermediate value.
#[tokio::test]
async fn test_bus_coalesces_progress_burst_into_one_event() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let bus = EventBus::new(tx);

    // 100k per-file progress updates, the shape of a large recursive copy
    // (ops.rs sends UpdateCurrentFile once per file, unthrottled).
    for i in 0..100_000 {
        bus.send(current_file(1, &format!("file{i}.bin")));
    }

    let events = drain(&mut rx);
    assert_eq!(
        snapshot_slots(events),
        vec![(1, TaskField::CurrentFile)],
        "a progress burst must collapse to a single notification"
    );
}

/// Coalescing must not lose the newest value: the UI has to end up with the
/// latest progress, not the first one it saw.
#[tokio::test]
async fn test_bus_keeps_latest_progress_value() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let bus = EventBus::new(tx);

    for i in 1..=500 {
        bus.send(update_progress(1, i));
    }

    assert_eq!(drain(&mut rx).len(), 1);
    assert!(matches!(
        bus.take_coalesced(1, TaskField::Progress),
        Some(TaskEvent::UpdateProgress {
            task_id: 1,
            processed: 500,
            total: 1_000_000,
        })
    ));
}

/// Different tasks and different fields must not collapse into each other.
#[tokio::test]
async fn test_bus_coalesces_per_task_and_field() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let bus = EventBus::new(tx);

    for i in 0..10 {
        bus.send(update_progress(1, i));
        bus.send(update_progress(2, i));
        bus.send(current_file(1, "a.bin"));
        bus.send(UiEvent::Task(TaskEvent::SetRsyncMode {
            task_id: 2,
            rsync: i % 2 == 0,
        }));
    }

    let mut snapshots = snapshot_slots(drain(&mut rx));
    snapshots.dedup();
    assert_eq!(
        snapshots,
        vec![
            (1, TaskField::Progress),
            (2, TaskField::Progress),
            (1, TaskField::CurrentFile),
            (2, TaskField::Rsync),
        ],
        "one notification per (task, field), queued in first-update order"
    );
    assert!(matches!(
        bus.take_coalesced(2, TaskField::Rsync),
        Some(TaskEvent::SetRsyncMode { rsync: false, .. })
    ));
    assert!(matches!(
        bus.take_coalesced(1, TaskField::Progress),
        Some(TaskEvent::UpdateProgress { processed: 9, .. })
    ));
}

/// Taking a value must re-arm the slot, so progress keeps flowing instead of
/// stalling on the first notification.
#[tokio::test]
async fn test_bus_slot_rearms_after_dispatch() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let bus = EventBus::new(tx);

    bus.send(update_progress(1, 1));
    assert_eq!(drain(&mut rx).len(), 1);
    assert!(matches!(
        bus.take_coalesced(1, TaskField::Progress),
        Some(TaskEvent::UpdateProgress { processed: 1, .. })
    ));

    bus.send(update_progress(1, 2));
    assert_eq!(
        snapshot_slots(drain(&mut rx)),
        vec![(1, TaskField::Progress)]
    );
    assert!(matches!(
        bus.take_coalesced(1, TaskField::Progress),
        Some(TaskEvent::UpdateProgress { processed: 2, .. })
    ));
}

/// One-shot events must never be dropped or reordered behind the coalesced
/// ones: conflict and error popups, SSH prompts and status changes all
/// depend on being observed exactly as sent.
#[tokio::test]
async fn test_bus_queues_one_shot_events_verbatim() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let bus = EventBus::new(tx);

    for i in 0..1_000 {
        bus.send(update_progress(1, i));
    }
    bus.send(UiEvent::Alert(AlertEvent::TaskError {
        task_id: 1,
        path: "/tmp/x".to_string(),
        message: "boom".to_string(),
    }));
    bus.send(UiEvent::Task(TaskEvent::UpdateStatus {
        task_id: 1,
        status: TaskStatus::Completed,
    }));

    let events = drain(&mut rx);
    assert_eq!(events.len(), 3, "one snapshot plus two one-shot events");
    assert!(matches!(events[0], UiEvent::TaskSnapshot { .. }));
    assert!(matches!(
        events[1],
        UiEvent::Alert(AlertEvent::TaskError { .. })
    ));
    // The status change stays behind the progress it terminates, so the UI
    // applies the final progress before marking the task finished.
    assert!(matches!(
        events[2],
        UiEvent::Task(TaskEvent::UpdateStatus {
            status: TaskStatus::Completed,
            ..
        })
    ));
    assert!(matches!(
        bus.take_coalesced(1, TaskField::Progress),
        Some(TaskEvent::UpdateProgress { processed: 999, .. })
    ));
}

/// `send` must stay usable after the event loop is gone.
#[tokio::test]
async fn test_bus_send_after_consumer_dropped() {
    let (tx, rx) = mpsc::unbounded_channel();
    let bus = EventBus::new(tx);
    drop(rx);

    assert!(bus.is_closed());
    bus.send(current_file(1, "a.bin"));
    bus.send(UiEvent::Task(TaskEvent::UpdateStatus {
        task_id: 1,
        status: TaskStatus::Completed,
    }));
    // The value stays pending, so a slot is never stranded without a way to
    // observe it if a consumer ever returns.
    assert!(matches!(
        bus.take_coalesced(1, TaskField::CurrentFile),
        Some(TaskEvent::UpdateCurrentFile { .. })
    ));
}

/// `TaskManager` hands the same coalescing bus to its workers, and resolves
/// snapshot notifications through it.
#[tokio::test]
async fn test_task_manager_resolves_snapshots() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tm = TaskManager::new(tx);
    let id = tm.spawn_task("task1", |_c, _tx, _id| async {});

    let bus = tm.get_tx();
    bus.send(update_progress(id, 7));
    bus.send(update_progress(id, 9));

    let UiEvent::TaskSnapshot { task_id, field } = drain(&mut rx).remove(0) else {
        panic!("expected a coalesced snapshot");
    };
    assert_eq!((task_id, field), (id, TaskField::Progress));

    let Some(TaskEvent::UpdateProgress { processed, .. }) = tm.take_coalesced(task_id, field)
    else {
        panic!("snapshot had no pending value");
    };
    tm.update_task_progress(id, processed, 1_000_000);
    assert_eq!(tm.get_tasks()[0].progress, Some((9, 1_000_000)));
}
