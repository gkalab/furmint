use fm::fs::fs_local::LocalFs;
use fm::fs::fs_provider::FileSystemProvider;
use fm::state::FileViewerState;
use std::sync::Arc;

#[tokio::test]
async fn test_large_file_no_duplication() {
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    let test_file = temp_file.path().to_path_buf();

    // Create a 15MB file with varying line lengths to trigger the bug we fixed
    let mut content = String::new();
    for i in 1..200000 {
        let len = (i % 50) + 10;
        content.push_str(&format!("Line {:06}: ", i));
        for _ in 0..len {
            content.push('X');
        }
        content.push('\n');
    }
    use std::io::Write;
    write!(temp_file, "{}", content).unwrap();

    let provider: Arc<dyn FileSystemProvider> = Arc::new(LocalFs::new());
    let mut state = FileViewerState::new(true, "catppuccin mocha");

    let limit = 5 * 1024 * 1024; // 5MB
    let file_size = std::fs::metadata(&test_file).unwrap().len();

    state.load_content(&test_file, &provider, Some(file_size), limit);

    assert!(state.large_file_reader.is_some());
    assert!(state.large_file_indexer.is_some());

    let indexer = state.large_file_indexer.as_ref().unwrap();
    let reader = state.large_file_reader.as_ref().unwrap();

    // Check for duplicates in a few ranges
    let check_ranges = [0..100, 100000..100100, 199800..199999];

    for range in check_ranges {
        let mut last_content = String::new();
        for i in range {
            if let Some((s, e)) = indexer.get_line_with_reader(i, reader) {
                let current_content = reader.get_chunk(s, e);
                // Lines should NOT be identical to the previous one
                assert_ne!(
                    current_content, last_content,
                    "Duplicate found at line {}",
                    i
                );

                // Verify the content matches the expected format "Line XXXXXX: ..."
                let expected_prefix = format!("Line {:06}:", i + 1);
                assert!(
                    current_content.starts_with(&expected_prefix),
                    "Line {} content mismatch: {:?}",
                    i,
                    current_content
                );

                last_content = current_content;
            }
        }
    }
}
