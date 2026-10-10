use fm::state::FileViewerState;
use std::str::FromStr;

/// Viewer geometry for a panel of the given size, mirroring what
/// `layout::compute_layout` produces for the viewer's side.
fn viewer_geometry(width: u16, height: u16) -> fm::layout::ViewerGeometry {
    let area = ratatui::layout::Rect::new(0, 0, width, height);
    fm::layout::ViewerGeometry {
        area,
        render_area: area.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        }),
    }
}

#[test]
fn test_display_col_to_char_idx() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["\tA\tB".to_string(), "Wide: 🚀".to_string()];

    // Tab at 0 expands to 4 spaces (0, 1, 2, 3). Display pos 4 is 'A'.
    assert_eq!(state.display_col_to_char_idx(0, 0), 0);
    assert_eq!(state.display_col_to_char_idx(0, 3), 0);
    assert_eq!(state.display_col_to_char_idx(0, 4), 1); // 'A'
    assert_eq!(state.display_col_to_char_idx(0, 5), 2); // Tab at index 2
    // Next tab starts at display pos 5. 5 % 4 = 1. Width = 4 - 1 = 3.
    // Positions 5, 6, 7 are the second tab.
    assert_eq!(state.display_col_to_char_idx(0, 7), 2);
    assert_eq!(state.display_col_to_char_idx(0, 8), 3); // 'B'

    // Wide character
    // "Wide: " is 6 chars. 🚀 is width 2.
    assert_eq!(state.display_col_to_char_idx(1, 5), 5); // ':'
    assert_eq!(state.display_col_to_char_idx(1, 6), 6); // '🚀' (first column)
    assert_eq!(state.display_col_to_char_idx(1, 7), 6); // '🚀' (second column)
    assert_eq!(state.display_col_to_char_idx(1, 8), 7); // End of line
}

#[test]
fn test_select_word_at_fallback() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["hello world_123 !!!".to_string()];
    state.text.language = lumis::languages::Language::default(); // No syntax

    // Select 'hello'
    state.select_word_at(0, 0);
    assert_eq!(state.text.selection, Some(((0, 0), (0, 5))));

    // Select 'world_123'
    state.select_word_at(0, 6);
    assert_eq!(state.text.selection, Some(((0, 6), (0, 15))));

    // Select '!!!' (non-word chunk)
    state.select_word_at(0, 16);
    assert_eq!(state.text.selection, Some(((0, 15), (0, 19))));
}

#[test]
fn test_select_word_at_quotes() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["\"\"\"triple\"\"\" '\"nested\"' `backtick`".to_string()];
    state.text.language = lumis::languages::Language::default();

    // Select triple
    // """triple""" is at 0-12. triple is at 3-9.
    state.select_word_at(0, 5);
    assert_eq!(state.text.selection, Some(((0, 3), (0, 9))));

    // Select nested
    // '"nested"' is at 13-22 (with space at 12).
    // ' (13), " (14), n (15) ... d (20), " (21), ' (22)
    state.select_word_at(0, 17);
    assert_eq!(state.text.selection, Some(((0, 15), (0, 21))));

    // Select backtick
    state.select_word_at(0, 28);
    assert_eq!(state.text.selection, Some(((0, 25), (0, 33))));

    // Single quote/double quote not matching -> select word only
    state.text.content = vec!["\"no match'".to_string()];
    state.select_word_at(0, 1);
    assert_eq!(state.text.selection, Some(((0, 1), (0, 3))));
}

#[test]
fn test_select_word_at_syntax_quotes() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.path = std::path::PathBuf::from("test.json");
    // Using from_str as it's safer for different lumis versions
    state.text.language = lumis::languages::Language::from_str("json").unwrap_or_default();
    state.text.content = vec!["{ \"key\": \"value\" }".to_string()];

    // index 10 is 'v' in "value"
    state.select_word_at(0, 10);
    // "value" is at indices 9-16. Refinement should strip quotes to 10-15.
    // NOTE: This test depends on lumis grouping "value" as a single segment.
    // If it doesn't, fallback will take over, which should also work.
    if let Some(((r, s), (re, e))) = state.text.selection {
        assert_eq!(r, 0);
        assert_eq!(re, 0);
        assert_eq!(s, 10);
        assert_eq!(e, 15);
    } else {
        panic!("Selection should not be None");
    }
}

/// Attaches a real highlight worker and returns the channel it reports on.
fn attach_highlight_worker(
    state: &mut FileViewerState,
) -> tokio::sync::mpsc::UnboundedReceiver<fm::state::HighlightBatch> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    state
        .text
        .set_highlight_worker(fm::state::HighlightWorker::start(tx));
    rx
}

/// The worker runs on its own thread and has to compile the language's queries
/// before it can answer, which is the whole point: the draw pass must not be
/// waiting on that. The generous timeout only guards against a hang.
const WORKER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// A line the worker has not returned yet renders as plain text rather than
/// blocking or coming out blank.
#[test]
fn test_viewer_renders_plain_text_before_highlighting() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.path = std::path::PathBuf::from("test.rs");
    state.text.language = lumis::languages::Language::from_str("rust").unwrap_or_default();
    state.text.content = vec!["fn main() {}".to_string()];
    let geometry = viewer_geometry(80, 20);

    assert!(state.text.line_segments(0).is_none(), "nothing cached yet");

    let rendered = render_viewer(&mut state);
    assert!(
        rendered.contains("fn main() {}"),
        "line should render unstyled, got:\n{rendered}"
    );
}
/// Renders the viewer into a test terminal and returns the screen as text.
fn render_viewer(state: &mut FileViewerState) -> String {
    render_viewer_cells(state).0
}

/// Renders the viewer and returns the screen text plus the foreground of every
/// non-blank cell inside the panel.
///
/// Colours are collected from the interior only: the border and the title are
/// drawn in their own colours and would mask what the content looks like.
fn render_viewer_cells(state: &mut FileViewerState) -> (String, Vec<ratatui::style::Color>) {
    let area = ratatui::layout::Rect::new(0, 0, 80, 20);
    let palette = fm::theme::get_theme("catppuccin macchiato").unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
    terminal
        .draw(|f| {
            fm::ui::draw_file_viewer(
                f,
                state,
                area,
                &viewer_geometry(80, 20),
                &palette,
                true,
                false,
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    let mut colors = Vec::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            text.push_str(buffer[(x, y)].symbol());
            let interior =
                y > area.top() && y < area.bottom() - 1 && x > area.left() && x < area.right() - 1;
            if interior && buffer[(x, y)].symbol() != " " {
                colors.push(buffer[(x, y)].fg);
            }
        }
        text.push('\n');
    }
    (text, colors)
}

/// The whole loop: the first frame paints plain text immediately, and once the
/// worker answers the same line comes back coloured.
#[tokio::test]
async fn test_viewer_colours_the_line_once_the_worker_answers() {
    const LINE: &str = "fn main() { let x: u32 = 42; }";

    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.path = std::path::PathBuf::from("test.rs");
    state.text.language = lumis::languages::Language::from_str("rust").unwrap_or_default();
    state.text.content = vec![LINE.to_string()];

    let mut rx = attach_highlight_worker(&mut state);

    // Before the worker has answered: the text is there, unstyled.
    let (plain_text, plain_colors) = render_viewer_cells(&mut state);
    assert!(
        plain_text.contains(LINE),
        "text must render before highlighting, got:\n{plain_text}"
    );

    // The draw pass asked for the line; the worker now compiles and answers.
    state.text.request_highlights(0, 1);
    let batch = tokio::time::timeout(WORKER_TIMEOUT, rx.recv())
        .await
        .expect("highlight worker never answered")
        .expect("highlight worker went away");
    state.text.apply_highlight_batch(batch);

    // Same text, now with more than one colour on screen.
    let (coloured_text, coloured_colors) = render_viewer_cells(&mut state);
    assert!(
        coloured_text.contains(LINE),
        "text must survive highlighting, got:\n{coloured_text}"
    );
    let distinct_plain = plain_colors
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let distinct_coloured = coloured_colors
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(distinct_plain.len(), 1, "plain frame should use one colour");
    assert!(
        distinct_coloured.len() > 1,
        "coloured frame should use several, got {distinct_coloured:?}"
    );
}

/// The end-to-end path: request lines, have the worker compile and highlight
/// them, and confirm the runs come back covering the line intact.
#[tokio::test]
async fn test_highlight_worker_returns_runs_covering_the_line() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.path = std::path::PathBuf::from("test.rs");
    state.text.language = lumis::languages::Language::from_str("rust").unwrap_or_default();
    state.text.content = vec!["fn main() { let x = 1; }".to_string()];

    let mut rx = attach_highlight_worker(&mut state);
    state.text.request_highlights(0, 1);

    let batch = tokio::time::timeout(WORKER_TIMEOUT, rx.recv())
        .await
        .expect("highlight worker never answered")
        .expect("highlight worker went away");

    state.text.apply_highlight_batch(batch);

    let runs = state
        .text
        .line_segments(0)
        .expect("line was not highlighted");
    let text: String = runs.iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(text, "fn main() { let x = 1; }", "runs must cover the line");
    assert!(
        runs.len() > 1,
        "expected several runs, got {}: {runs:?}",
        runs.len()
    );
    assert!(
        runs.iter().any(|(color, _)| color.is_some()),
        "expected at least one coloured run with a theme"
    );
}

/// A warm request compiles the queries without producing runs, so the first
/// real request for a language does not have to.
#[tokio::test]
async fn test_warm_request_produces_no_batch() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.language = lumis::languages::Language::from_str("python").unwrap_or_default();

    let mut rx = attach_highlight_worker(&mut state);
    state.text.warm_highlight();

    // Nothing to receive: a warm request must not send a batch. Give the worker
    // a moment to (incorrectly) answer, then prove nothing arrived.
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(250), rx.recv())
            .await
            .is_err(),
        "a warm request must not produce a batch"
    );
}

#[test]
fn test_search_basic() {
    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = vec![
        "hello world".to_string(),
        "this is me".to_string(),
        "another line".to_string(),
        "me too".to_string(),
    ];

    state.search("me", &geometry);
    // "this is me" is at line 1. Visible, so no scroll.
    assert_eq!(state.text.current_search_match, Some((1, 8, 10)));
    assert_eq!(state.text.scroll_offset, 0);

    state.search_next(&geometry);
    // "me too" is at line 3. Visible, so no scroll.
    assert_eq!(state.text.current_search_match, Some((3, 0, 2)));
    assert_eq!(state.text.scroll_offset, 0);

    // Move viewport away
    state.text.scroll_offset = 10;
    state.search_next(&geometry);
    // Wrap around to line 1. NOT visible (1 < 10), so jump with context.
    assert_eq!(state.text.current_search_match, Some((1, 8, 10)));
    assert_eq!(state.text.scroll_offset, 0); // 1 - 4
}

#[test]
fn test_search_regex() {
    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = vec!["abc123def".to_string(), "ghi456jkl".to_string()];

    state.search("\\d+", &geometry);
    assert_eq!(state.text.current_search_match, Some((0, 3, 6)));

    state.search_next(&geometry);
    assert_eq!(state.text.current_search_match, Some((1, 3, 6)));
}

#[test]
fn test_search_case_insensitive() {
    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = vec!["Hello".to_string(), "world".to_string()];

    state.search("hello", &geometry);
    assert_eq!(state.text.current_search_match, Some((0, 0, 5)));
}

#[test]
fn test_search_empty() {
    let mut state = FileViewerState::new(true, "test");
    state.text.content = vec!["abc".to_string()];
    let geometry = viewer_geometry(80, 20);
    state.search("", &geometry);
    assert_eq!(state.text.current_search_match, None);
}

#[test]
fn test_search_large_file_chunked() {
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    let test_file = temp_file.path().to_path_buf();

    // ~3.3MB / 100k lines so the search crosses 2MB decode chunks.
    let mut content = String::new();
    for i in 0..100_000 {
        match i {
            500 => content.push_str("first marker here"),
            50_000 => content.push_str("middle marker here"),
            99_999 => content.push_str("last marker here"),
            _ => content.push_str("filler line for testing purposes"),
        }
        content.push('\n');
    }
    std::fs::write(&test_file, &content).unwrap();

    let reader =
        fm::large_text::file_reader::FileReader::new(test_file, encoding_rs::UTF_8).unwrap();
    let mut indexer = fm::large_text::line_indexer::LineIndexer::new();
    indexer.index_file(&reader);

    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = Vec::new();
    state.text.large_file_reader = Some(reader);
    state.text.large_file_indexer = Some(indexer);

    state.search("marker", &geometry);
    assert_eq!(state.text.current_search_match, Some((500, 6, 12)));

    state.search_next(&geometry);
    assert_eq!(state.text.current_search_match, Some((50_000, 7, 13)));

    state.search_next(&geometry);
    assert_eq!(state.text.current_search_match, Some((99_999, 5, 11)));

    state.search_next(&geometry); // wraps to top
    assert_eq!(state.text.current_search_match, Some((500, 6, 12)));

    state.search_prev(&geometry); // wraps to bottom
    assert_eq!(state.text.current_search_match, Some((99_999, 5, 11)));

    state.search_prev(&geometry);
    assert_eq!(state.text.current_search_match, Some((50_000, 7, 13)));

    // No match: full-file scan.
    assert!(!state.search("nomatch", &geometry));
    assert_eq!(state.text.current_search_match, None);
}

#[test]
fn test_search_prev() {
    let mut state = FileViewerState::new(true, "test");
    let geometry = viewer_geometry(80, 20);
    state.text.content = vec![
        "match 1".to_string(),
        "match 2".to_string(),
        "match 3".to_string(),
    ];
    state.text.scroll_offset = 2;
    state.search("match", &geometry);
    // Starts from scroll_offset 2, so finds "match 3" at line 2.
    assert_eq!(state.text.current_search_match, Some((2, 0, 5)));

    state.search_prev(&geometry);
    assert_eq!(state.text.current_search_match, Some((1, 0, 5)));

    state.search_prev(&geometry);
    assert_eq!(state.text.current_search_match, Some((0, 0, 5)));

    state.search_prev(&geometry); // Wrap to bottom
    assert_eq!(state.text.current_search_match, Some((2, 0, 5)));
}
