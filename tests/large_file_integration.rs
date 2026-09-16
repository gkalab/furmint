use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::FileSystemProvider;
use fm::state::FileViewerState;
use std::sync::Arc;

#[tokio::test]
async fn test_large_file_no_duplication() {
    use std::fmt::Write as FmtWrite;
    use std::io::Write as IoWrite;
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    let test_file = temp_file.path().to_path_buf();

    // Create a 15MB file with varying line lengths to trigger the bug we fixed
    let mut content = String::new();
    for i in 1..200_000 {
        let len = (i % 50) + 10;
        let _ = write!(content, "Line {i:06}: ");
        for _ in 0..len {
            content.push('X');
        }
        content.push('\n');
    }
    write!(temp_file, "{content}").unwrap();

    let provider: Arc<dyn FileSystemProvider> = Arc::new(LocalFs::new());
    let mut state = FileViewerState::new(true, "catppuccin mocha");

    let limit = 5 * 1024 * 1024; // 5MB
    let file_size = std::fs::metadata(&test_file).unwrap().len();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<fm::state::ContentLoadResult>();
    state.text.set_content_load_channel(tx);

    state
        .load_content(&test_file, &provider, Some(file_size), limit)
        .await;

    // Indexing runs in a background task; collect its result
    let res = tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv())
        .await
        .expect("timed out waiting for large file index")
        .expect("channel closed");
    state.handle_content_load_result(res);

    assert!(state.text.large_file_reader.is_some());
    assert!(state.text.large_file_indexer.is_some());

    let indexer = state.text.large_file_indexer.as_ref().unwrap();
    let reader = state.text.large_file_reader.as_ref().unwrap();

    // Check for duplicates in a few ranges
    let check_ranges = [0..100, 100_000..100_100, 199_800..199_999];

    for range in check_ranges {
        let mut last_content = String::new();
        for i in range {
            if let Some((s, e)) = indexer.get_line_with_reader(i, reader) {
                let current_content = reader.get_chunk(s, e);
                // Lines should NOT be identical to the previous one
                assert_ne!(current_content, last_content, "Duplicate found at line {i}");

                // Verify the content matches the expected format "Line XXXXXX: ..."
                let expected_prefix = format!("Line {:06}:", i + 1);
                assert!(
                    current_content.starts_with(&expected_prefix),
                    "Line {i} content mismatch: {current_content:?}"
                );

                last_content = current_content;
            }
        }
    }
}
