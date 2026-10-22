use super::*;

/// An app whose panels show two different hosts over SSH.
fn two_remote_panels(left_host: &'static str, right_host: &'static str) -> AppState {
    let mut app = AppState::test_default();
    app.panels.left.tabs = vec![remote_tab(left_host)];
    app.panels.right.tabs = vec![remote_tab(right_host)];
    app
}

/// Marks both panels as idle again, as if their listing had come back.
fn settle(app: &mut AppState) {
    app.panels.left.active_tab_mut().is_reloading = false;
    app.panels.right.active_tab_mut().is_reloading = false;
}

#[tokio::test]
async fn throttled_reload_rate_limits_each_context() {
    let mut app = two_remote_panels("alpha", "beta");

    app.reload_remote_throttled();
    assert!(app.panels.left.active_tab().is_reloading);
    assert!(app.panels.right.active_tab().is_reloading);

    // A second tick right away is within the rate limit of both contexts.
    settle(&mut app);
    app.reload_remote_throttled();
    assert!(!app.panels.left.active_tab().is_reloading);
    assert!(!app.panels.right.active_tab().is_reloading);

    // The right panel is pointed at a host that was never reloaded: its
    // refresh must not be held back by the left panel's rate limit.
    app.panels.right.tabs[0] = remote_tab("gamma");
    app.reload_remote_throttled();
    assert!(!app.panels.left.active_tab().is_reloading);
    assert!(app.panels.right.active_tab().is_reloading);
}

#[tokio::test]
async fn unthrottled_reload_ignores_the_rate_limit() {
    let mut app = two_remote_panels("alpha", "beta");

    app.reload_remote_throttled();
    settle(&mut app);

    app.reload_remote();
    assert!(app.panels.left.active_tab().is_reloading);
    assert!(app.panels.right.active_tab().is_reloading);
}

#[tokio::test]
async fn throttled_reload_skips_local_panels() {
    let mut app = AppState::test_default();

    app.reload_remote_throttled();
    assert!(!app.panels.left.active_tab().is_reloading);
    assert!(!app.panels.right.active_tab().is_reloading);
}

#[tokio::test]
async fn reload_skips_tabs_already_reloading() {
    let mut app = two_remote_panels("alpha", "beta");
    app.panels.right.active_tab_mut().is_reloading = true;

    app.reload_remote_throttled();
    assert!(app.panels.left.active_tab().is_reloading);
    assert!(app.panels.right.active_tab().is_reloading);
}
