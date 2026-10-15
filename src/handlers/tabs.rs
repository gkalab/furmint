//! Tab management event handlers for new, next, previous, and close tab.

use crate::app::AppState;
use crate::fs::local::LocalFs;
use crate::fs::provider::FileSystemProvider;
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

    {
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
    // The new tab was listed with the default (hidden shown); reconcile it.
    app.apply_show_hidden();
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
mod tests;
