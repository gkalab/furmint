//! Regression tests: the optimized transfer (e.g. archive extraction) must be
//! probed by capability and executed only after the conflict decision.

use fm::fs::fs_archive::ArchiveFs;
use fm::fs::fs_local::LocalFs;
use fm::fs::ops::{DecisionState, RecursiveOpContext, recursive_op};
use fm::state::CopyMoveAction;
use fm::tasks::{AlertEvent, TaskDecision, UiEvent};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};

fn make_archive_with_payload(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("payload/", opts).unwrap();
    zip.start_file("payload/file.txt", opts).unwrap();
    zip.write_all(b"from-archive").unwrap();
    zip.finish().unwrap();
}

struct Harness {
    dest_file: PathBuf,
    tx: UnboundedSender<UiEvent>,
    decision_tx: Sender<TaskDecision>,
    decision_rx: Arc<tokio::sync::Mutex<Receiver<TaskDecision>>>,
    cancel: Arc<AtomicBool>,
    processed: Arc<AtomicUsize>,
    processed_bytes: Arc<AtomicU64>,
}

/// Sets up an archive -> local copy of the `payload` directory where the
/// destination directory already exists and contains a file.
fn setup(
    archive_path: &Path,
    dest_root: &Path,
    tx: UnboundedSender<UiEvent>,
) -> (ArchiveFs, LocalFs, Harness) {
    let archive = ArchiveFs::new(archive_path).unwrap();
    let local = LocalFs::new();

    std::fs::create_dir_all(dest_root.join("payload")).unwrap();
    let dest_file = dest_root.join("payload").join("file.txt");
    std::fs::write(&dest_file, "existing").unwrap();

    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel::<TaskDecision>(1);
    (
        archive,
        local,
        Harness {
            dest_file,
            tx,
            decision_tx,
            decision_rx: Arc::new(tokio::sync::Mutex::new(decision_rx)),
            cancel: Arc::new(AtomicBool::new(false)),
            processed: Arc::new(AtomicUsize::new(0)),
            processed_bytes: Arc::new(AtomicU64::new(0)),
        },
    )
}

fn build_ctx<'a>(
    archive: &'a ArchiveFs,
    local: &'a LocalFs,
    dest: &'a Path,
    h: &'a Harness,
) -> RecursiveOpContext<'a> {
    RecursiveOpContext {
        src_fs: archive,
        dest_fs: local,
        src: Path::new("payload"),
        dest,
        action: CopyMoveAction::Copy,
        cancel: &h.cancel,
        tx: &h.tx,
        id: 1,
        total: 2,
        total_bytes: 0,
        processed: &h.processed,
        processed_bytes: &h.processed_bytes,
        decision_rx: &h.decision_rx,
    }
}

fn conflict_alerts(events: &[UiEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, UiEvent::Alert(AlertEvent::Conflict { .. })))
        .count()
}

/// Runs the whole directory copy with the given conflict decision and returns
/// (events, processed item count, transferred bytes, final destination content).
async fn run_copy_with(decision: TaskDecision) -> (Vec<UiEvent>, usize, u64, String) {
    let tmp = tempfile::tempdir().unwrap();
    let archive_path = tmp.path().join("payload.zip");
    make_archive_with_payload(&archive_path);
    let dest_root = tmp.path().join("dest");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let (archive, local, h) = setup(&archive_path, &dest_root, tx);

    let dest = dest_root.join("payload");
    let ctx = build_ctx(&archive, &local, &dest, &h);
    h.decision_tx.send(decision).await.unwrap();
    recursive_op(ctx, &mut DecisionState::new()).await.unwrap();

    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    let content = std::fs::read_to_string(&h.dest_file).unwrap();
    (
        events,
        h.processed.load(std::sync::atomic::Ordering::Relaxed),
        h.processed_bytes.load(std::sync::atomic::Ordering::Relaxed),
        content,
    )
}

/// Copying a directory out of an archive over an existing directory: the
/// transfer must only happen after the user decides. Skip must leave the
/// destination completely untouched and transfer no bytes.
#[tokio::test]
async fn skip_does_not_overwrite_destination() {
    let (events, processed, bytes, content) = run_copy_with(TaskDecision::Skip).await;

    assert_eq!(conflict_alerts(&events), 1, "one conflict prompt expected");
    assert_eq!(
        content, "existing",
        "skip must not overwrite the destination"
    );
    assert_eq!(processed, 1, "only the skipped item is accounted for");
    assert_eq!(bytes, 0, "no transfer may happen before the decision");
}

/// With Overwrite the transfer runs — and runs exactly once: the payload's
/// 12 bytes are transferred a single time, not twice.
#[tokio::test]
async fn overwrite_transfers_exactly_once() {
    let (events, _processed, bytes, content) = run_copy_with(TaskDecision::Overwrite).await;

    assert_eq!(conflict_alerts(&events), 1, "one conflict prompt expected");
    assert_eq!(content, "from-archive", "overwrite must copy the payload");
    assert_eq!(bytes, 12, "transfer must run exactly once");
}
