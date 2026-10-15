use super::*;
use crate::app_state::tabs::TabManager;
use secrecy::SecretString;
use std::path::Path;

async fn test_app() -> AppState {
    crate::test_utils::TestAppBuilder::new()
        .left(TabManager::new(Path::new(".")).await.unwrap())
        .right(TabManager::new(Path::new(".")).await.unwrap())
        .build()
}

#[tokio::test]
async fn test_close_tab_cleans_up_ssh_session() {
    let mut app = test_app().await;

    app.panels.left.new_tab(Path::new("."), None).await.unwrap();
    let session_id = "ssh_testhost_22_1".to_string();
    app.panels.left.active_tab_mut().ssh_session_id = Some(session_id.clone());
    app.ssh_manager.register_session(
        session_id.clone(),
        "testhost".to_string(),
        22,
        "user".to_string(),
        None,
        crate::ssh::manager::AuthMethod::Password,
    );
    app.ssh_manager
        .cache_password(&session_id, SecretString::new("secret".to_string().into()));

    assert!(app.ssh_manager.get_session(&session_id).is_some());
    assert!(app.ssh_manager.get_cached_password(&session_id).is_some());

    handle_close_tab(&mut app).await;

    assert_eq!(app.panels.left.tabs.len(), 1);
    assert!(app.ssh_manager.get_session(&session_id).is_none());
    assert!(app.ssh_manager.get_cached_password(&session_id).is_none());
}

#[tokio::test]
async fn test_close_tab_keeps_session_when_provider_shared() {
    let mut app = test_app().await;

    let provider = app.panels.left.tabs[0].provider.clone();
    app.panels
        .left
        .new_tab_with_provider(Path::new("."), provider, None)
        .await
        .unwrap();
    let session_id = "ssh_testhost_22_1".to_string();
    app.panels.left.active_tab_mut().ssh_session_id = Some(session_id.clone());
    app.panels.left.tabs[0].ssh_session_id = Some(session_id.clone());
    app.ssh_manager.register_session(
        session_id.clone(),
        "testhost".to_string(),
        22,
        "user".to_string(),
        None,
        crate::ssh::manager::AuthMethod::Password,
    );

    handle_close_tab(&mut app).await;

    assert_eq!(app.panels.left.tabs.len(), 1);
    assert!(app.ssh_manager.get_session(&session_id).is_some());
}

#[tokio::test]
async fn test_close_tab_without_session_id_is_noop() {
    let mut app = test_app().await;

    app.panels.left.new_tab(Path::new("."), None).await.unwrap();
    assert!(app.panels.left.active_tab_mut().ssh_session_id.is_none());

    handle_close_tab(&mut app).await;

    assert_eq!(app.panels.left.tabs.len(), 1);
}

#[tokio::test]
async fn test_new_tab_inherits_ssh_session_id() {
    let mut app = test_app().await;

    app.panels.left.active_tab_mut().ssh_session_id = Some("ssh_testhost_22_1".to_string());

    handle_new_tab(&mut app).await;

    assert_eq!(app.panels.left.tabs.len(), 2);
    assert_eq!(
        app.panels
            .left
            .tabs
            .last()
            .unwrap()
            .ssh_session_id
            .as_deref(),
        Some("ssh_testhost_22_1")
    );
}
