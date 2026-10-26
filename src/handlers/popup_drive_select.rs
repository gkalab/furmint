//! Drive-select popup event handler.

use crate::app::AppState;
use crate::app_state::tabs::PanelSide;
use termina::event::KeyCode;

pub async fn handle_drive_select_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Escape => {
            app.popups.reset_popup(crate::app::PopupKind::DriveSelect);
        }
        KeyCode::Down if !app.popups.drive_select.drives.is_empty() => {
            app.popups.drive_select.selected_index =
                (app.popups.drive_select.selected_index + 1) % app.popups.drive_select.drives.len();
        }
        KeyCode::Up if !app.popups.drive_select.drives.is_empty() => {
            if app.popups.drive_select.selected_index == 0 {
                app.popups.drive_select.selected_index = app.popups.drive_select.drives.len() - 1;
            } else {
                app.popups.drive_select.selected_index -= 1;
            }
        }
        KeyCode::Char(c) => {
            // Typing a drive letter jumps straight to it (drive-letter paths
            // are Windows-only; Linux mount paths never match "X:\").
            let target = format!("{}:\\", c.to_ascii_uppercase());
            if let Some(drive) = app
                .popups
                .drive_select
                .drives
                .iter()
                .find(|d| d.path.to_ascii_uppercase() == target)
                .cloned()
            {
                perform_drive_navigation(app, drive.path).await;
            }
        }
        KeyCode::Enter => {
            if let Some(drive) = app
                .popups
                .drive_select
                .drives
                .get(app.popups.drive_select.selected_index)
                .cloned()
            {
                perform_drive_navigation(app, drive.path).await;
            }
        }
        _ => {}
    }
    false
}

async fn perform_drive_navigation(app: &mut AppState, path: String) {
    let side = app.popups.drive_select.side;
    #[cfg(windows)]
    let mut path = std::path::PathBuf::from(path);
    #[cfg(not(windows))]
    let path = std::path::PathBuf::from(path);

    // Windows: selecting the drive the opposite panel is already on keeps that
    // panel's directory. Linux drives are mount points and the exact selected
    // mount path is used.
    #[cfg(windows)]
    {
        let get_drive = |p: &std::path::Path| -> Option<char> {
            p.components().find_map(|c| {
                if let std::path::Component::Prefix(prefix) = c {
                    match prefix.kind() {
                        std::path::Prefix::Disk(d) | std::path::Prefix::VerbatimDisk(d) => {
                            Some((d as char).to_ascii_uppercase())
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
        };

        if let Some(sel_drive) = get_drive(&path) {
            let (opp_drive, opp_dir) = {
                let opposite_tab = match side {
                    PanelSide::Left => app.panels.right.active_tab(),
                    PanelSide::Right => app.panels.left.active_tab(),
                };

                let opp_drive = if opposite_tab.provider.is_local() {
                    get_drive(&opposite_tab.current_dir)
                } else {
                    None
                };

                (opp_drive, opposite_tab.current_dir.clone())
            };

            if opp_drive == Some(sel_drive) {
                path = opp_dir;
            }
        }
    }

    let success = {
        let tab_manager = match side {
            PanelSide::Left => &mut app.panels.left,
            PanelSide::Right => &mut app.panels.right,
        };
        tab_manager
            .active_tab_mut()
            .navigate_to(&path)
            .await
            .is_ok()
    };

    if success {
        app.popups.reset_popup(crate::app::PopupKind::DriveSelect);
        app.sync_watcher();
    }
}
