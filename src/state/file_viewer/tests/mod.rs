use crate::state::FileViewerState;
use ratatui::style::Color;
use std::str::FromStr;

/// Viewer geometry for a panel of the given size, mirroring what
/// `layout::compute_layout` produces for the viewer's side.
fn viewer_geometry(width: u16, height: u16) -> crate::layout::ViewerGeometry {
    let area = ratatui::layout::Rect::new(0, 0, width, height);
    crate::layout::ViewerGeometry {
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
) -> tokio::sync::mpsc::UnboundedReceiver<crate::state::HighlightBatch> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    state
        .text
        .set_highlight_worker(crate::state::HighlightWorker::start(tx));
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
    let _geometry = viewer_geometry(80, 20);

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
    render_viewer_cells_with_icons(state, false)
}

/// Renders the viewer with icons enabled and returns the screen as text.
fn render_viewer_with_icons(state: &mut FileViewerState) -> String {
    render_viewer_cells_with_icons(state, true).0
}

fn render_viewer_cells_with_icons(
    state: &mut FileViewerState,
    icons: bool,
) -> (String, Vec<ratatui::style::Color>) {
    let area = ratatui::layout::Rect::new(0, 0, 80, 20);
    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
    terminal
        .draw(|f| {
            crate::ui::draw_file_viewer(
                f,
                state,
                area,
                &viewer_geometry(80, 20),
                &palette,
                true,
                icons,
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

/// Renders the viewer and returns the background color of every cell, indexed
/// `[y][x]`, so selection highlights can be inspected.
fn render_viewer_bg_cells(state: &mut FileViewerState, icons: bool) -> Vec<Vec<Color>> {
    let area = ratatui::layout::Rect::new(0, 0, 80, 20);
    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
    terminal
        .draw(|f| {
            crate::ui::draw_file_viewer(
                f,
                state,
                area,
                &viewer_geometry(80, 20),
                &palette,
                true,
                icons,
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    (0..area.height)
        .map(|y| (0..area.width).map(|x| buffer[(x, y)].bg).collect())
        .collect()
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
        crate::large_text::file_reader::FileReader::new(&test_file, encoding_rs::UTF_8).unwrap();
    let mut indexer = crate::large_text::line_indexer::LineIndexer::new();
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

const VIEWER_LIMIT: u64 = 10 * 1024 * 1024;

/// A directory preview starts with the directory's full name and lists its
/// children below it, directories first (as `list_dir` orders them).
#[tokio::test]
async fn test_directory_listing_content() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("zeta")).unwrap();
    std::fs::write(temp.path().join("alpha.txt"), "hi").unwrap();

    let provider: std::sync::Arc<dyn crate::fs::provider::FileSystemProvider> =
        std::sync::Arc::new(crate::fs::local::LocalFs::new());
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state
        .load_content(temp.path(), &provider, None, VIEWER_LIMIT, true)
        .await;

    let expected_header = temp.path().to_string_lossy().to_string();
    assert_eq!(state.text.content[0], expected_header);
    assert_eq!(state.text.content[1], "zeta");
    assert_eq!(state.text.content[2], "alpha.txt");
}

/// When hidden files are hidden, the directory preview omits them, matching the
/// tab listing; when shown, they appear like any other entry.
#[tokio::test]
async fn test_directory_listing_respects_hidden_flag() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("alpha.txt"), "hi").unwrap();
    std::fs::write(temp.path().join(".hidden.txt"), "shh").unwrap();
    std::fs::create_dir(temp.path().join(".hidden_dir")).unwrap();

    let provider: std::sync::Arc<dyn crate::fs::provider::FileSystemProvider> =
        std::sync::Arc::new(crate::fs::local::LocalFs::new());

    let mut hidden = FileViewerState::new(true, "catppuccin macchiato");
    hidden
        .load_content(temp.path(), &provider, None, VIEWER_LIMIT, false)
        .await;
    assert_eq!(hidden.text.content.len(), 2, "header plus one visible file");
    assert_eq!(hidden.text.content[1], "alpha.txt");
    assert!(
        !hidden.text.content.iter().any(|l| l.contains("hidden")),
        "hidden entries must be withheld: {:?}",
        hidden.text.content
    );

    let mut shown = FileViewerState::new(true, "catppuccin macchiato");
    shown
        .load_content(temp.path(), &provider, None, VIEWER_LIMIT, true)
        .await;
    assert!(shown.text.content.iter().any(|l| l == ".hidden.txt"));
    assert!(shown.text.content.iter().any(|l| l == ".hidden_dir"));
}

/// The plain content is only the names; the icons are drawn by the renderer
/// when icons are enabled.
#[test]
fn test_directory_listing_renders_icons() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["/tmp/dir".to_string(), "zeta".to_string()];
    state.text.directory_rows = Some(vec![header_row("/tmp/dir"), dir_row("zeta")]);

    let rendered = render_viewer_with_icons(&mut state);
    assert!(
        rendered.contains('󰉋'),
        "a directory entry must carry its folder icon, got:\n{rendered}"
    );
    assert!(rendered.contains("zeta"));
}

/// Entries are colored with the same foregrounds as a tab listing: directories
/// blue, regular files the default text color, and symlinks cyan.
#[test]
fn test_directory_listing_uses_panel_colors() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec![
        "/tmp/dir".to_string(),
        "subdir".to_string(),
        "plain.txt".to_string(),
        "link".to_string(),
    ];
    state.text.directory_rows = Some(vec![
        header_row("/tmp/dir"),
        dir_row("subdir"),
        file_row("plain.txt"),
        symlink_row("link"),
    ]);

    let (_, colors) = render_viewer_cells(&mut state);
    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let has = |rgb: crate::theme::Rgb| colors.contains(&Color::Rgb(rgb.r, rgb.g, rgb.b));
    assert!(has(palette.blue), "directories must be blue: {colors:?}");
    assert!(has(palette.text), "regular files must use the text color");
    assert!(has(palette.cyan), "symlinks must be cyan");
}

/// Directory-listing rows draw an icon before the text stored in `content`, so
/// mouse hit-testing needs to know how many columns to skip.
#[test]
fn test_directory_listing_rendered_row_prefix() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["/tmp/dir".to_string(), "zeta".to_string()];
    state.text.directory_rows = Some(vec![header_row("/tmp/dir"), dir_row("zeta")]);

    // The header draws no icon, and neither does any row when icons are off.
    assert_eq!(state.rendered_row_prefix(0, true), 0);
    assert_eq!(state.rendered_row_prefix(0, false), 0);
    assert_eq!(state.rendered_row_prefix(1, false), 0);
    // Out-of-range rows have no prefix.
    assert_eq!(state.rendered_row_prefix(9, true), 0);

    let icon_width = crate::icons::get_icon("zeta", true, false, false)
        .chars()
        .count();
    assert_eq!(state.rendered_row_prefix(1, true), icon_width + 1);
}

/// A text selection on a directory listing line is rendered with the same
/// background color the text viewer uses.
#[test]
fn test_directory_listing_renders_text_selection() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["/tmp/dir".to_string(), "alpha.txt".to_string()];
    state.text.directory_rows = Some(vec![header_row("/tmp/dir"), file_row("alpha.txt")]);
    // Select the entry name (content characters 0..9) on the second line.
    state.text.selection = Some(((1, 0), (1, 9)));

    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let surface0 = Color::Rgb(palette.surface0.r, palette.surface0.g, palette.surface0.b);

    let bgs = render_viewer_bg_cells(&mut state, false);
    // Interior starts at (1, 1); content line 1 is screen row 2.
    assert_eq!(
        bgs[2][1], surface0,
        "selection must start at the first char"
    );
    assert_eq!(bgs[2][9], surface0, "selection covers the whole name");
    assert_ne!(bgs[2][10], surface0, "selection must stop at the name");
    assert_ne!(bgs[1][1], surface0, "the header stays unselected");
}

/// With icons enabled the selection highlight is shifted past the icon so it
/// still lines up with the name.
#[test]
fn test_directory_listing_selection_accounts_for_icon_offset() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec!["/tmp/dir".to_string(), "alpha.txt".to_string()];
    state.text.directory_rows = Some(vec![header_row("/tmp/dir"), file_row("alpha.txt")]);
    state.text.selection = Some(((1, 0), (1, 9)));

    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let surface0 = Color::Rgb(palette.surface0.r, palette.surface0.g, palette.surface0.b);

    let offset = state.rendered_row_prefix(1, true);
    assert!(
        offset >= 2,
        "an entry icon plus its space is at least 2 cells"
    );

    let bgs = render_viewer_bg_cells(&mut state, true);
    let first = 1 + offset;
    assert_eq!(
        bgs[2][first], surface0,
        "selection starts after the icon, not on it"
    );
    assert_eq!(bgs[2][first + 8], surface0, "selection covers the name");
    assert_ne!(bgs[2][1], surface0, "the icon itself is not highlighted");
}

/// Ctrl+C copies the selected lines; for a directory listing that means the
/// entry names exactly as shown, without any icon.
#[test]
fn test_directory_listing_selection_copies_names() {
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state.text.content = vec![
        "/tmp/dir".to_string(),
        "zeta".to_string(),
        "alpha.txt".to_string(),
    ];
    state.text.directory_rows = Some(vec![
        header_row("/tmp/dir"),
        dir_row("zeta"),
        file_row("alpha.txt"),
    ]);
    state.text.selection = Some(((1, 0), (2, 9)));

    assert_eq!(
        state.get_selected_text().as_deref(),
        Some("zeta\nalpha.txt")
    );
}

fn header_row(name: &str) -> crate::state::DirectoryListingRow {
    crate::state::DirectoryListingRow {
        is_header: true,
        entry: listing_entry(name, true, false),
        hidden: false,
        executable: false,
        link_target: None,
    }
}

/// A directory listing row for a child directory.
fn dir_row(name: &str) -> crate::state::DirectoryListingRow {
    entry_row(name, true, false)
}

/// A directory listing row for a regular file.
fn file_row(name: &str) -> crate::state::DirectoryListingRow {
    entry_row(name, false, false)
}

/// A directory listing row for a symlink.
fn symlink_row(name: &str) -> crate::state::DirectoryListingRow {
    entry_row(name, false, true)
}

/// A non-header row. These tests don't exercise the hidden or executable flags,
/// so both stay unset.
fn entry_row(name: &str, is_dir: bool, is_symlink: bool) -> crate::state::DirectoryListingRow {
    crate::state::DirectoryListingRow {
        is_header: false,
        entry: listing_entry(name, is_dir, is_symlink),
        hidden: false,
        executable: false,
        link_target: None,
    }
}

/// A bare entry for a listing row, with the metadata fields the draw pass does
/// not read left unset.
fn listing_entry(name: &str, is_dir: bool, is_symlink: bool) -> crate::fs::utils::FileEntry {
    crate::fs::utils::FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    }
}

/// Selecting a symlinked directory shows its link target next to its name.
#[cfg(unix)]
#[tokio::test]
async fn test_directory_listing_shows_link_target() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("child.txt"), "hi").unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let provider: std::sync::Arc<dyn crate::fs::provider::FileSystemProvider> =
        std::sync::Arc::new(crate::fs::local::LocalFs::new());
    let mut state = FileViewerState::new(true, "catppuccin macchiato");
    state
        .load_content(&link, &provider, None, VIEWER_LIMIT, true)
        .await;

    let expected = format!("{} -> {}", link.to_string_lossy(), real.to_string_lossy());
    assert_eq!(state.text.content[0], expected);
    // The link lists its target's contents.
    assert_eq!(state.text.content[1], "child.txt");
}
