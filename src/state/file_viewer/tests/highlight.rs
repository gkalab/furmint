use super::*;

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
