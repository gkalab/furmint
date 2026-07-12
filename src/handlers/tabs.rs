//! Tab management event handlers for new, next, previous, and close tab.

use crate::app::AppState;
use crate::fs::fs_local::LocalFs;
use crate::fs::fs_provider::FileSystemProvider;
use crate::handlers::navigation::update_viewer_content;
use directories::UserDirs;
use std::sync::Arc;

pub(crate) fn handle_new_tab(app: &mut AppState) {
    let (target_dir, provider, cursor) = {
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
            )
        } else {
            (
                tab.current_dir.clone(),
                tab.provider.clone(),
                Some(tab.cursor),
            )
        }
    };

    let tab_manager = app.active_tab_manager_mut();
    if let Err(e) = tab_manager.new_tab_with_provider(&target_dir, provider, cursor) {
        tab_manager.active_tab_mut().error = Some(format!("Error creating tab: {e}"));
    }
}

pub(crate) fn handle_next_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();
    tab_manager.next_tab();
    let _ = tab_manager.active_tab_mut().reload();
    update_viewer_content(app);
}

pub(crate) fn handle_prev_tab(app: &mut AppState) {
    let tab_manager = app.active_tab_manager_mut();
    tab_manager.prev_tab();
    let _ = tab_manager.active_tab_mut().reload();
    update_viewer_content(app);
}

pub(crate) fn handle_close_tab(app: &mut AppState) {
    let current_index = app.active_tab_manager().active_tab_index;
    let is_local = app.active_tab().provider.context_key() == "local";

    if is_local && app.active_tab_manager().local_tab_count() <= 1 {
        app.active_tab_mut().error = Some("Cannot close the last local tab".to_string());
        return;
    }

    if !app.active_tab_manager_mut().close_tab(current_index) {
        // Could not close (last tab)
    }
    update_viewer_content(app);
}

pub(crate) fn handle_move_tab(app: &mut AppState, target_side: crate::app_state::tabs::PanelSide) {
    if let Err(e) = app.move_active_tab_to_other_side(target_side) {
        app.active_tab_mut().error = Some(e.to_string());
    } else {
        update_viewer_content(app);
    }
}
