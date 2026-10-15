use super::*;
use crate::fs::archive::preview::ArchiveTreeRow;
use crate::theme::catppuccin_macchiato;
use unicode_width::UnicodeWidthStr;

fn test_palette() -> crate::theme::ThemePalette {
    catppuccin_macchiato()
}

#[test]
fn test_archive_search_highlight() {
    let mut viewer = FileViewerState::new(true, "catppuccin macchiato");
    viewer.text.archive_rows = Some(vec![ArchiveTreeRow {
        prefix: "└─ ".to_string(),
        name: "src".to_string(),
        is_dir: true,
        size: None,
    }]);
    // Content line is "└─ src"; match "src" at chars 3-6.
    viewer.text.current_search_match = Some((0, 3, 6));

    let ctx = ViewerLineContext {
        line_idx: 0,
        viewer: &viewer,
        is_large_file: false,
        segments: None,
        default_fg: None,
        normalized_selection: None,
        max_width: 100,
        palette: &test_palette(),
        icons_enabled: true,
    };

    let line = render_viewer_line(&ctx).expect("line should render");
    let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
    assert_eq!(text, "└─ 󰉋 src");

    // The highlighted "src" span should have yellow bg and base fg.
    let highlighted = line
        .spans
        .iter()
        .find(|s| s.content == "src")
        .expect("src span");
    assert_eq!(
        highlighted.style.bg,
        Some(Color::Rgb(
            test_palette().yellow.r,
            test_palette().yellow.g,
            test_palette().yellow.b
        ))
    );
    assert_eq!(
        highlighted.style.fg,
        Some(Color::Rgb(
            test_palette().base.r,
            test_palette().base.g,
            test_palette().base.b
        ))
    );
}

#[test]
fn test_archive_line_hides_icons_when_disabled() {
    let mut viewer = FileViewerState::new(true, "catppuccin macchiato");
    viewer.text.archive_rows = Some(vec![ArchiveTreeRow {
        prefix: "└─ ".to_string(),
        name: "main.rs".to_string(),
        is_dir: false,
        size: Some(1024),
    }]);

    let ctx = ViewerLineContext {
        line_idx: 0,
        viewer: &viewer,
        is_large_file: false,
        segments: None,
        default_fg: None,
        normalized_selection: None,
        max_width: 100,
        palette: &test_palette(),
        icons_enabled: false,
    };

    let line = render_viewer_line(&ctx).expect("line should render");
    let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.starts_with("└─ main.rs"), "got: {text}");
    assert!(!line.spans.iter().any(|s| s.content == "󰉋"));
}

#[test]
fn test_rendering_overflow_prevention() {
    // Simulate a viewport width
    let max_width = 10;
    let h_offset = 0;

    // Dummy colour for testing
    let dummy_color = Some(Color::Rgb(0xff, 0xff, 0xff));

    // Test cases that would overflow if tabs were counted as 1 char but rendered as 4 spaces
    // or if unicode width wasn't handled correctly.
    let test_cases = vec![
        // Case 1: Tabs expanding.
        // '\t' (4 spaces) + '\t' (4 spaces) + "ABC" (3 chars) = 11 width.
        // Char count = 5. If we only checked char count (5 < 10), this would overflow.
        ("\t\tABC", "Two tabs and text"),
        // Case 2: Mixed content just over the limit
        // "1234567890" (10 chars) + "1" = 11 width.
        ("12345678901", "Simple overflow"),
        // Case 3: Tab crossing the boundary
        // "12345678" (8 chars) + "\t" (4 chars -> pos 12).
        ("12345678\t", "Tab crossing boundary"),
    ];

    for (input, description) in test_cases {
        // Treat the whole line as one run for baseline testing
        let segments = vec![(dummy_color, input)];

        let spans = generate_line_spans(&LineSpansContext {
            segments: &segments,
            h_offset,
            max_width,
            default_fg: None,
            selection: None,
            selection_bg: None,
            search_ranges: &[],
            search_bg: None,
            search_fg: None,
        });

        // Calculate total display width of the generated spans
        let mut total_width = 0;
        let mut resulting_text = String::new();
        for span in &spans {
            // Note: generate_line_spans converts tabs to spaces, so width() works here
            total_width += span.content.width();
            resulting_text.push_str(&span.content);
        }

        println!("Test Case: {description}");
        println!("  Input: {input:?}");
        println!("  Result: '{resulting_text}'");
        println!("  Display width: {total_width}");

        // Assert that the total width does not exceed max_width
        assert!(
            total_width <= max_width,
            "Overflow detected for '{description}'! Width: {total_width}, Max: {max_width}. Result: '{resulting_text}'"
        );
    }
}

/// Tabs must be emitted as spaces, not raw tab bytes: the terminal would
/// not expand them the way the viewer's 4-column hit-testing expects,
/// which would shift text left of where the selection assumes it is.
#[test]
fn test_tabs_render_as_spaces() {
    let segments = vec![(Some(Color::Rgb(0, 0, 0)), "\tABC\tDE")];
    let spans = generate_line_spans(&LineSpansContext {
        segments: &segments,
        h_offset: 0,
        max_width: 100,
        default_fg: None,
        selection: None,
        selection_bg: None,
        search_ranges: &[],
        search_bg: None,
        search_fg: None,
    });

    // First tab expands to 4 spaces; the second sits at column 7 and
    // advances to the next 4-stop (column 8), so it is a single space.
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert_eq!(text, "    ABC DE");
}

/// Adjacent runs that resolve to the same style must merge into one span,
/// across run boundaries, without losing the text between them.
#[test]
fn test_runs_with_equal_style_merge() {
    let segments = vec![
        (Some(Color::Rgb(1, 2, 3)), "aaa"),
        (Some(Color::Rgb(1, 2, 3)), "bbb"),
        (Some(Color::Rgb(9, 9, 9)), "ccc"),
        (None, "ddd"),
    ];

    let spans = generate_line_spans(&LineSpansContext {
        segments: &segments,
        h_offset: 0,
        max_width: 100,
        default_fg: Some(Color::Rgb(7, 7, 7)),
        selection: None,
        selection_bg: None,
        search_ranges: &[],
        search_bg: None,
        search_fg: None,
    });

    let text: String = spans.iter().map(|span| span.content.to_string()).collect();
    assert_eq!(text, "aaabbbcccddd", "text must survive run merging");
    assert_eq!(spans.len(), 3, "expected one span per distinct style");
    assert_eq!(spans[0].content, "aaabbb");
    assert_eq!(spans[2].style.fg, Some(Color::Rgb(7, 7, 7)));
}
