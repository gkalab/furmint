use super::*;

fn batch(generation: usize, lines: Vec<(usize, LineSegments)>) -> crate::state::HighlightBatch {
    crate::state::HighlightBatch { generation, lines }
}

#[test]
fn stale_generations_are_dropped() {
    let mut state = TextViewerState::default();
    state.apply_highlight_batch(batch(0, vec![(0, vec![(None, "kept".to_string())])]));
    assert_eq!(state.line_segments(0).map(<[_]>::len), Some(1));

    // A new file bumps the generation, so results for the old one no longer
    // apply and a late batch must not colour the new one.
    state.reset();
    state.apply_highlight_batch(batch(0, vec![(0, vec![(None, "stale".to_string())])]));
    assert!(state.line_segments(0).is_none(), "stale batch was applied");

    state.apply_highlight_batch(batch(1, vec![(0, vec![(None, "fresh".to_string())])]));
    assert_eq!(
        state.line_segments(0).map(|runs| runs[0].1.as_str()),
        Some("fresh")
    );
}

#[test]
fn plain_text_lines_need_no_worker() {
    let mut state = TextViewerState {
        content: vec!["one".to_string(), "two".to_string(), "three".to_string()],
        ..Default::default()
    };

    // No worker attached: plain text must still resolve to runs, or the draw
    // pass would keep asking for them.
    state.request_highlights(0, 2);

    assert_eq!(
        state.line_segments(0).map(|runs| runs[0].1.as_str()),
        Some("one")
    );
    assert_eq!(
        state.line_segments(1).map(|runs| runs[0].1.as_str()),
        Some("two")
    );
    assert!(state.line_segments(2).is_none(), "outside the window");
}

#[test]
fn cache_is_pruned_around_the_viewport() {
    let mut state = TextViewerState {
        content: (0..5000).map(|i| i.to_string()).collect(),
        ..Default::default()
    };

    state.request_highlights(0, 10);
    assert!(state.line_segments(0).is_some());

    // Jump far away; the old runs must not be kept forever.
    state.request_highlights(4000, 4010);
    assert!(state.line_segments(0).is_none(), "stale run was retained");
    assert!(state.line_segments(4005).is_some());
}

#[test]
fn syntax_selection_follows_the_cached_runs() {
    let mut state = TextViewerState {
        content: vec!["a + b".to_string()],
        ..Default::default()
    };
    state.apply_highlight_batch(batch(
        0,
        vec![(
            0,
            vec![
                (None, "a".to_string()),
                (None, " ".to_string()),
                (None, "+".to_string()),
                (None, " ".to_string()),
                (None, "b".to_string()),
            ],
        )],
    ));

    // Clicking the operator selects just the operator, which only the
    // syntax runs can do: word boundaries would take the spaces too.
    state.select_word_at(0, 2);
    assert_eq!(state.selection, Some(((0, 2), (0, 3))));
}

#[test]
fn wrapping_breaks_at_spaces() {
    let lines = vec!["the quick brown fox".to_string()];
    assert_eq!(wrap_lines(&lines, 10), vec!["the quick", "brown fox"]);
}

#[test]
fn wrapping_hard_breaks_long_words() {
    let lines = vec!["abcdefghij".to_string()];
    assert_eq!(wrap_lines(&lines, 4), vec!["abcd", "efgh", "ij"]);
}

#[test]
fn wrapping_preserves_blank_lines() {
    let lines = vec!["a".to_string(), String::new(), "b".to_string()];
    assert_eq!(wrap_lines(&lines, 10), vec!["a", "", "b"]);
}

/// Wrapped content starts out as the unwrapped source and is re-flowed once
/// the draw pass reports the width, then again on resize.
#[test]
fn wrapped_content_refows_on_width_change() {
    let mut state = TextViewerState::default();
    state.set_wrapped_content(vec!["the quick brown fox".to_string()]);

    assert!(state.is_wrapped());
    assert_eq!(
        state.content,
        vec!["the quick brown fox"],
        "seeded unwrapped"
    );

    state.rewrap(10);
    assert_eq!(state.content, vec!["the quick", "brown fox"]);

    state.rewrap(20);
    assert_eq!(state.content, vec!["the quick brown fox"]);

    state.reset();
    assert!(!state.is_wrapped());
}
