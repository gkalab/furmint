use super::*;

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
