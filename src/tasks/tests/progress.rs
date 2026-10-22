use super::*;

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
