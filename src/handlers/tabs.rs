//! Tab management event handlers for new, next, previous, and close tab.

use crate::app::AppState;
use crate::fs::fs_local::LocalFs;
use crate::fs::fs_provider::FileSystemProvider;
use crate::handlers::navigation::update_viewer_content;
use directories::UserDirs;
use std::sync::Arc;

pub(crate) async fn handle_new_tab(app: &mut AppState) {
    let (target_dir, provider, cursor, ssh_session_id) = {
        let tab = app.active_tab();
        if tab.is_archive() {
            let archive_file_path = tab.provider.archive_path();
            let parent_dir = archive_file_path
                .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
                .unwrap_or_else(|| {
                    UserDirs::new().map_or_else(
                        || std::path::PathBuf::from("."),
                        |u| u.home_dir().to_path_buf(),
                    )
                });
            (
                parent_dir,
                Arc::new(LocalFs::new()) as Arc<dyn FileSystemProvider>,
                None,
                None,
            )
        } else {
            (
                tab.current_dir.clone(),
                tab.provider.clone(),
                Some(tab.cursor),
                tab.ssh_session_id.clone(),
            )
        }
    };

    let tab_manager = app.active_tab_manager_mut();
    if let Err(e) = tab_manager
        .new_tab_with_provider(&target_dir, provider, cursor)
        .await
    {
        tab_manager.active_tab_mut().error = Some(format!("Error creating tab: {e}"));
    } else {
        tab_manager.active_tab_mut().ssh_session_id = ssh_session_id;
    }
}

pub(crate) async fn handle_next_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();
    tab_manager.next_tab();
    let _ = tab_manager.active_tab_mut().reload().await;
    update_viewer_content(app).await;
}

pub(crate) async fn handle_prev_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();
    tab_manager.prev_tab();
    let _ = tab_manager.active_tab_mut().reload().await;
    update_viewer_content(app).await;
}

pub(crate) async fn handle_close_tab(app: &mut AppState) {
    let current_index = app.active_tab_manager().active_tab_index;
    let is_local = app.active_tab().provider.context_key().is_local();

    if is_local && app.active_tab_manager().local_tab_count() <= 1 {
        app.active_tab_mut().error = Some("Cannot close the last local tab".to_string());
        return;
    }

    let (session_id, provider) = {
        let tab = app.active_tab();
        (tab.ssh_session_id.clone(), tab.provider.clone())
    };

    if app.active_tab_manager_mut().close_tab(current_index) {
        cleanup_closed_ssh_tab(app, session_id, &provider);
    }
    update_viewer_content(app).await;
}

/// Unregisters the SSH session and drops the cached password of a closed tab,
/// unless another tab still shares the same provider connection.
pub(crate) fn cleanup_closed_ssh_tab(
    app: &mut AppState,
    session_id: Option<String>,
    provider: &Arc<dyn FileSystemProvider>,
) {
    let Some(session_id) = session_id else {
        return;
    };
    let shared = app
        .panels
        .left
        .tabs
        .iter()
        .chain(app.panels.right.tabs.iter())
        .any(|t| Arc::ptr_eq(&t.provider, provider));
    if shared {
        return;
    }
    app.ssh_manager.unregister_session(&session_id);
    app.ssh_manager.clear_password(&session_id);
}

pub(crate) async fn handle_move_tab(
    app: &mut AppState,
    target_side: crate::app_state::tabs::PanelSide,
) {
    if let Err(e) = app.move_active_tab_to_other_side(target_side) {
        app.active_tab_mut().error = Some(e.to_string());
    } else {
        update_viewer_content(app).await;
    }
}

#[cfg(test)]
mod tests {
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
            crate::ssh_manager::AuthMethod::Password,
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
            crate::ssh_manager::AuthMethod::Password,
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
}
