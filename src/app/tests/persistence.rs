use super::*;

fn empty_persistent_state(show_hidden: bool) -> crate::app::PersistentState {
    crate::app::PersistentState {
        left: crate::app_state::tabs::PersistentPanel {
            tabs: vec![],
            active_tab_index: 0,
        },
        right: crate::app_state::tabs::PersistentPanel {
            tabs: vec![],
            active_tab_index: 0,
        },
        active_side: PanelSide::Left,
        show_hidden,
    }
}

#[test]
fn persistent_state_round_trips_show_hidden() {
    for value in [true, false] {
        let json = serde_json::to_string(&empty_persistent_state(value)).unwrap();
        let restored: crate::app::PersistentState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.show_hidden, value);
    }
}

#[test]
fn persistent_state_written_before_the_toggle_still_shows_hidden() {
    // A state file from an older build has no `show_hidden` key. It must load as
    // "show hidden", which was the only behaviour back then.
    let legacy = r#"{
        "left": {"tabs": [], "active_tab_index": 0},
        "right": {"tabs": [], "active_tab_index": 0},
        "active_side": "Left"
    }"#;
    let restored: crate::app::PersistentState = serde_json::from_str(legacy).unwrap();
    assert!(restored.show_hidden);
}
