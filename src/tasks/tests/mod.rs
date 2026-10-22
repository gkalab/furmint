use super::*;
use tokio::sync::mpsc;

mod events;
mod manager;
mod progress;

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
