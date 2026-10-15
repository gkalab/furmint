use super::*;

/// The keys the help screen shows for `label`, if the label is listed.
fn keys_for(label: &str) -> Option<String> {
    build_help_categories(&crate::config::default_keyboard_config())
        .into_iter()
        .flat_map(|(_, bindings)| bindings)
        .find(|(l, _)| *l == label)
        .map(|(_, keys)| {
            keys.as_ref()
                .map_or_else(|| "None".to_string(), |k| k.join(", "))
        })
}

#[test]
fn toggle_hidden_is_listed_with_its_default_shortcut() {
    assert_eq!(keys_for("Toggle Hidden Files"), Some("Alt-.".to_string()));
}

#[test]
fn toggle_hidden_shows_none_when_unbound() {
    let keyboard = crate::config::KeyboardConfig::default();
    let listed = build_help_categories(&keyboard)
        .into_iter()
        .flat_map(|(_, bindings)| bindings)
        .any(|(label, keys)| label == "Toggle Hidden Files" && keys.is_none());
    assert!(listed);
}
