use super::*;

#[test]
fn test_task_progress_truncation() {
    let palette = crate::theme::catppuccin_macchiato();

    // Case 1: No truncation needed
    // Progress strings: "50% (5 left) " (13) + "| " (2) + "[50% of 1000 B] " (~16) = ~31 overhead
    // "file.txt " = 9 chars. Total needed ~40.
    let max_width = 60;
    let spans = get_task_progress_spans(
        Some((5, 10)),
        Some((500, 1000)),
        false,
        Some("file.txt"),
        max_width,
        &palette,
    );
    let total_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    assert!(total_width <= max_width);
    assert!(spans.iter().any(|s| s.content.contains("file.txt")));

    // Case 2: Truncation needed for long filename
    // Overhead ~31. Max width 45. Available ~14.
    // File "very...txt" > 14. Should truncate.
    let case2_width = 45;
    let long_file = "very_long_filename_that_definitely_needs_truncation.txt";
    let spans = get_task_progress_spans(
        Some((5, 10)),
        Some((500, 1000)),
        false,
        Some(long_file),
        case2_width,
        &palette,
    );
    let total_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    assert!(total_width <= case2_width);
    // The filename should be truncated
    assert!(spans.iter().any(|s| s.content.contains("…")));

    // Case 3: Very narrow width
    // Max width 10. Overhead ~31. File should be dropped.
    // Total width will exceed max_width because progress bars are not truncated,
    // but we verify file is dropped.
    let narrow_width = 10;
    let spans = get_task_progress_spans(
        Some((5, 10)),
        Some((500, 1000)),
        false,
        Some("short.txt"),
        narrow_width,
        &palette,
    );
    // We expect the file to be absent
    assert!(!spans.iter().any(|s| s.content.contains("short.txt")));
}
