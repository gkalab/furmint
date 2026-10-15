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
