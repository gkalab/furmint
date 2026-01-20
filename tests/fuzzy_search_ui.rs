use fm::ui_utils;
use std::path::Path;

#[test]
fn test_fuzzy_search_truncation_integration() {
    let path = Path::new("/a/very/long/path/that/needs/truncation");
    let max_width = 20;
    let truncated = ui_utils::truncate_path_with_ellipsis(path, max_width);

    assert!(truncated.len() <= max_width + 10);
    assert!(truncated.contains("…"));
}
