use super::*;

/// The scratch name embeds the destination, the pid and the sequence number,
/// so it is stable for one sequence and distinct across sequences. A pure
/// function of `dst` (the old scheme) let two writers share one partial file.
#[test]
fn test_temp_copy_path_is_unique_per_sequence() {
    let dst = Path::new("/some/dir/report.bin");

    let a = temp_copy_path(dst, 1).unwrap();
    let b = temp_copy_path(dst, 2).unwrap();
    let a_again = temp_copy_path(dst, 1).unwrap();

    assert_ne!(a, b, "different sequences must yield different names");
    assert_eq!(a, a_again, "the same sequence must be reproducible");
    assert!(a.starts_with("/some/dir"));
    let name = a.file_name().unwrap().to_str().unwrap();
    assert!(
        name.contains("report.bin"),
        "name must name the destination"
    );
    assert!(
        std::path::Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("part")),
        "name must mark a partial file"
    );
    assert!(
        name.contains(&std::process::id().to_string()),
        "name must carry the writer's pid"
    );

    assert!(
        temp_copy_path(Path::new("/"), 0).is_none(),
        "a destination with no parent has no scratch directory"
    );
}

/// Two writers reserving scratch space for the same destination get distinct,
/// already-created files, so neither can corrupt the other's partial copy.
#[tokio::test]
async fn test_reserve_temp_copy_is_unique_per_writer() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("shared.bin");

    let a = fs().reserve_temp_copy(&dst).await.unwrap().unwrap();
    let b = fs().reserve_temp_copy(&dst).await.unwrap().unwrap();

    assert_ne!(a, b, "two writers must not share one scratch file");
    assert_ne!(a, dst, "the scratch file is not the destination");
    assert!(
        a.exists() && b.exists(),
        "a reserved scratch file is created"
    );
    assert_eq!(
        leftover_temps(dir.path(), "shared.bin").len(),
        2,
        "both reserved scratch files are discoverable as partials of dst"
    );
}

/// Reserving is exclusive: an existing name is reported as taken (and left
/// untouched) rather than being truncated out from under its writer.
#[tokio::test]
async fn test_create_file_exclusive_refuses_existing_name() {
    let dir = tempfile::tempdir().unwrap();
    let occupied = dir.path().join(".shared.bin.1-0.part");
    std::fs::write(&occupied, b"existing").unwrap();

    assert!(
        !fs().create_file_exclusive(&occupied).await.unwrap(),
        "an existing name must not be re-created"
    );
    assert_eq!(
        std::fs::read(&occupied).unwrap(),
        b"existing",
        "an existing scratch file must not be truncated"
    );

    let fresh = dir.path().join(".shared.bin.1-1.part");
    assert!(
        fs().create_file_exclusive(&fresh).await.unwrap(),
        "a free name must be created"
    );
    assert_eq!(std::fs::read(&fresh).unwrap(), b"");
}

/// A copy whose first scratch candidate is refused must step over it and still
/// commit the destination; the step-over is observable as more than one
/// exclusive-create attempt.
#[tokio::test]
async fn test_copy_steps_over_a_taken_scratch_name() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("data.bin");
    let dst = dir.path().join("data_copy.bin");
    std::fs::write(&src, b"payload").unwrap();

    let mut flaky = flaky_copy_fs(usize::MAX, usize::MAX);
    flaky.refuse_creates = 1;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = crate::tasks::EventBus::new(tx);
    let cancel = Arc::new(AtomicBool::new(false));
    flaky
        .copy_with_progress(&src, &dst, &copy_progress(&bus, &cancel, 0))
        .await
        .unwrap();

    assert_eq!(std::fs::read(&dst).unwrap(), b"payload");
    assert!(
        flaky.creates.load(std::sync::atomic::Ordering::Relaxed) >= 2,
        "the copy must step over a taken scratch name, not reuse it"
    );
    assert!(
        leftover_temps(dir.path(), "data_copy.bin").is_empty(),
        "a stepped-over name must leave nothing behind"
    );
}
