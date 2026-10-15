use super::super::*;
use crate::test_utils::StdFileSystem;
use std::fs::{self, File};
use std::path::PathBuf;

#[tokio::test]
async fn test_count_items_empty() {
    let empty: Vec<PathBuf> = vec![];
    assert_eq!(count_items(&StdFileSystem, &empty).await, 0);
}

#[tokio::test]
async fn test_count_items_files_and_dirs() {
    // Setup temp dir structure: tmpdir/ (file1, subdir/file2, subdir2/)
    let tmp_dir = tempfile::tempdir().unwrap();
    let file1 = tmp_dir.path().join("file1.txt");
    File::create(&file1).unwrap();
    let subdir = tmp_dir.path().join("subdir");
    fs::create_dir(&subdir).unwrap();
    let file2 = subdir.join("file2.txt");
    File::create(&file2).unwrap();
    let subdir2 = tmp_dir.path().join("subdir2");
    fs::create_dir(&subdir2).unwrap();

    // Paths to test: root of tmp_dir only
    let root_entries: Vec<PathBuf> = fs::read_dir(tmp_dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    // Should count: file1.txt, subdir, subdir2, file2.txt
    assert_eq!(count_items(&StdFileSystem, &root_entries).await, 4);
}

#[cfg(unix)]
#[tokio::test]
async fn test_copy_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let tmp_dir = tempfile::tempdir().unwrap();
    let src_file = tmp_dir.path().join("source.txt");
    let dst_file = tmp_dir.path().join("destination.txt");

    // Create source file with specific permissions
    {
        std::fs::write(&src_file, b"test content").unwrap();
        // Set executable permissions
        std::process::Command::new("chmod")
            .args(["755", &src_file.to_string_lossy()])
            .status()
            .unwrap();
    }

    // Verify source has correct permissions
    let src_perms = std::fs::metadata(&src_file).unwrap().permissions().mode() & 0o777;
    assert_eq!(src_perms, 0o755);

    // Copy the file
    StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

    // Verify destination file exists and has same permissions
    assert!(dst_file.exists());
    let dst_perms = std::fs::metadata(&dst_file).unwrap().permissions().mode() & 0o777;
    assert_eq!(dst_perms, 0o755, "Expected 0o755, got 0{dst_perms:o}");
}

#[tokio::test]
async fn test_copy_preserves_modified_time() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let src_file = tmp_dir.path().join("source.txt");
    let dst_file = tmp_dir.path().join("destination.txt");

    std::fs::write(&src_file, b"test content").unwrap();

    let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_hours(447_072);
    assert!(StdFileSystem.set_modified_time(&src_file, past_time).await);

    let src_mtime = StdFileSystem.get_modified_time(&src_file).await;
    assert!(src_mtime.is_some());

    StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

    assert!(dst_file.exists());
    let dst_mtime = StdFileSystem.get_modified_time(&dst_file).await;

    if let Some(dst_mtime) = dst_mtime {
        let diff = dst_mtime.duration_since(past_time).unwrap();
        assert!(
            diff.as_secs() < 2,
            "Timestamp not preserved within 2 seconds"
        );
    }
}

#[tokio::test]
async fn test_set_modified_time() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let test_file = tmp_dir.path().join("test_mtime.txt");

    std::fs::write(&test_file, b"test").unwrap();

    let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_hours(447_072);
    assert!(StdFileSystem.set_modified_time(&test_file, past_time).await);

    let mtime = StdFileSystem.get_modified_time(&test_file).await;
    assert!(mtime.is_some());

    if let Some(mtime) = mtime {
        let diff = mtime.duration_since(past_time).unwrap();
        assert!(diff.as_secs() < 2, "Modified time not set correctly");
    }

    let content = std::fs::read(&test_file).unwrap();
    assert_eq!(
        content, b"test",
        "File content should be preserved after set_modified_time"
    );
}

/// The conflict prompt's answers, normalised once for every receiver.
#[test]
fn conflict_prompt_decision_protocol() {
    use crate::tasks::TaskDecision as D;
    let conflict = |d| Decision::from_task_decision(DecisionKind::Conflict, d);

    assert_eq!(conflict(Some(D::Overwrite)), Decision::Overwrite);
    assert_eq!(conflict(Some(D::OverwriteAll)), Decision::OverwriteAll);
    assert_eq!(conflict(Some(D::Skip)), Decision::Skip);
    assert_eq!(conflict(Some(D::SkipAll)), Decision::SkipAll);
    assert_eq!(conflict(Some(D::Cancel)), Decision::Cancel);

    // 'm' merges: the walker has already descended into the destination, so a
    // merge proceeds rather than silently discarding the source.
    assert_eq!(conflict(Some(D::Merge)), Decision::Overwrite);

    // The error prompt's Retry is not a conflict answer; it stops the item
    // instead of being mistaken for an overwrite.
    assert_eq!(conflict(Some(D::Retry)), Decision::Skip);
    // A dropped channel (the task is gone) reads as skip.
    assert_eq!(conflict(None), Decision::Skip);
}

/// The error prompt's answers, normalised once for every receiver.
#[test]
fn error_prompt_decision_protocol() {
    use crate::tasks::TaskDecision as D;
    let error = |d| Decision::from_task_decision(DecisionKind::Error, d);

    assert_eq!(error(Some(D::Retry)), Decision::Retry);
    assert_eq!(error(Some(D::Skip)), Decision::Skip);
    assert_eq!(error(Some(D::SkipAll)), Decision::SkipAll);
    assert_eq!(error(Some(D::Cancel)), Decision::Cancel);

    // Conflict-only answers must never overwrite a file from the error path.
    assert_eq!(error(Some(D::Overwrite)), Decision::Skip);
    assert_eq!(error(Some(D::OverwriteAll)), Decision::Skip);
    assert_eq!(error(Some(D::Merge)), Decision::Skip);
    assert_eq!(error(None), Decision::Skip);
}

#[tokio::test]
async fn test_copy_preserves_content_and_modified_time() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let src_file = tmp_dir.path().join("source.txt");
    let dst_file = tmp_dir.path().join("destination.txt");

    let original_content = b"Hello, World! This is a test file with some content.";
    std::fs::write(&src_file, original_content).unwrap();

    let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_hours(447_072);
    assert!(StdFileSystem.set_modified_time(&src_file, past_time).await);

    StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

    let dst_content = std::fs::read(&dst_file).unwrap();
    assert_eq!(
        dst_content, original_content,
        "Copied file content should match source"
    );

    let dst_mtime = StdFileSystem.get_modified_time(&dst_file).await.unwrap();
    let diff = dst_mtime.duration_since(past_time).unwrap();
    assert!(diff.as_secs() < 2, "Timestamp should be preserved");
}
