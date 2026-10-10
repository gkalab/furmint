use crate::app::AppState;
use crate::state::{ConfirmationAction, ConfirmationState};
use std::path::PathBuf;
use std::time::Instant;
use termina::event::{KeyCode, Modifiers};

/// Refresh the visible bookmark list (items + precomposed display strings) from
/// the current filter query.
pub fn refresh_bookmark_list(app: &mut AppState) {
    let query = app.popups.bookmark.list.input.clone();
    let entries = app.bookmark_store.fuzzy_search_entries(&query);
    app.popups.bookmark.list.items = entries
        .iter()
        .map(|e| {
            crate::ui::filterable_list::ListItem::with_display(
                e.path.clone(),
                crate::bookmarks::BookmarkEntry::display_string(e),
            )
        })
        .collect();
}

/// The selected `BookmarkEntry`, resolved from the current filter results.
fn selected_bookmark_entry(app: &AppState) -> Option<crate::bookmarks::BookmarkEntry> {
    let query = app.popups.bookmark.list.input.clone();
    let idx = app.popups.bookmark.list.selected_index;
    app.bookmark_store
        .fuzzy_search_entries(&query)
        .into_iter()
        .nth(idx)
}

pub async fn handle_bookmark_mouse_click(
    app: &mut AppState,
    x: u16,
    y: u16,
    is_double_click: bool,
) {
    if app.popups.bookmark.confirmation.is_some() {
        return;
    }
    let state = &mut app.popups.bookmark.list;
    let Some(list_area) = state.list_area else {
        return;
    };
    if !crate::handlers::mouse::is_in_rect((x, y), list_area) {
        return;
    }

    let row = (y - list_area.y) as usize + state.scroll_offset;
    if row >= state.items.len() {
        return;
    }

    state.selected_index = row;

    if is_double_click {
        handle_bookmark_event(KeyCode::Enter, Modifiers::NONE, app).await;
    }
}

pub fn handle_bookmark_add(app: &mut AppState) {
    let tab = app.active_tab();
    let provider = tab.provider.clone();

    let (ssh_user, ssh_host, ssh_port) = match provider.context_key() {
        crate::fs::provider::ContextKey::Ssh { user, host, port } => {
            let port = (port != 22).then_some(port);
            (Some(user), Some(host), port)
        }
        _ => (None, None, None),
    };

    // An archive tab's `current_dir` is a path *inside* the archive, which cannot
    // be navigated to later. Bookmark the archive file itself instead.
    let archive_path = if tab.is_archive() {
        provider.archive_path()
    } else {
        None
    };
    let entry = crate::bookmarks::BookmarkEntry {
        path: archive_path
            .clone()
            .unwrap_or_else(|| tab.current_dir.clone()),
        ssh_user,
        ssh_host,
        ssh_port,
        is_archive: archive_path.is_some(),
    };

    let display = entry.display_string();

    match app.bookmark_store.add_entry(entry) {
        Ok(true) => {
            app.active_tab_mut().status_msg =
                Some((format!("Bookmark added: {display}"), Instant::now()));
        }
        Ok(false) => {
            app.active_tab_mut().status_msg =
                Some((format!("Already bookmarked: {display}"), Instant::now()));
        }
        Err(e) => {
            app.active_tab_mut().error = Some(format!("Failed to save bookmark: {e}"));
        }
    }
}

pub async fn handle_bookmark_event(
    code: KeyCode,
    modifiers: Modifiers,
    app: &mut AppState,
) -> bool {
    // 1. Handle confirmation overlay if active
    if let Some(conf) = &mut app.popups.bookmark.confirmation {
        use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
        match get_choice_with_selection(code, &mut conf.selected_no) {
            ChoiceResult::Confirmed => {
                if let ConfirmationAction::DeleteBookmark(idx) = conf.action {
                    if let Err(e) = app.bookmark_store.remove(idx) {
                        app.active_tab_mut().error = Some(format!("Failed to save bookmarks: {e}"));
                    }
                    // Refresh the filtered list
                    refresh_bookmark_list(app);
                    // Adjust selection if it's now out of bounds
                    if app.popups.bookmark.list.selected_index
                        >= app.popups.bookmark.list.items.len()
                    {
                        app.popups.bookmark.list.selected_index =
                            app.popups.bookmark.list.items.len().saturating_sub(1);
                    }
                }
                app.popups.bookmark.confirmation = None;
            }
            ChoiceResult::Cancelled => {
                app.popups.bookmark.confirmation = None;
            }
            ChoiceResult::None => {}
        }
        return false;
    }

    // 2. Main bookmark list events
    match code {
        KeyCode::Escape => {
            app.popups
                .set_popup_visible(crate::app::PopupKind::Bookmark, false);
            app.popups.reset_popup(crate::app::PopupKind::Bookmark);
        }
        KeyCode::Enter => {
            handle_bookmark_enter(app).await;
        }
        KeyCode::Up => {
            app.popups.bookmark.list.move_selection_up();
        }
        KeyCode::Down => {
            app.popups.bookmark.list.move_selection_down();
        }
        KeyCode::PageUp => {
            app.popups.bookmark.list.move_selection_page_up(10);
        }
        KeyCode::PageDown => {
            app.popups.bookmark.list.move_selection_page_down(10);
        }
        KeyCode::Delete => {
            if let Some(entry) = selected_bookmark_entry(app) {
                let path_display =
                    crate::ui::ui_utils::truncate_path_str(&entry.display_string(), 50);
                // We need the index in the actual store to delete it, but the list might be filtered.
                // Match all identifying fields: the same path+host can exist with
                // different user or port, which are distinct bookmarks.
                if let Some(store_idx) = app.bookmark_store.entries.iter().position(|e| {
                    e.path == entry.path
                        && e.ssh_user == entry.ssh_user
                        && e.ssh_host == entry.ssh_host
                        && e.ssh_port == entry.ssh_port
                        && e.is_archive == entry.is_archive
                }) {
                    app.popups.bookmark.confirmation = Some(ConfirmationState::new(
                        format!("Remove bookmark '{path_display}'?"),
                        true,
                        ConfirmationAction::DeleteBookmark(store_idx),
                    ));
                }
            }
        }
        _ => {
            if crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut app.popups.bookmark.list.input,
                &mut app.popups.bookmark.list.cursor_position,
                false,
            ) {
                refresh_bookmark_list(app);
                app.popups.bookmark.list.selected_index = 0;
                app.popups.bookmark.list.scroll_offset = 0;
            }
        }
    }
    false
}

async fn handle_bookmark_enter(app: &mut AppState) {
    if let Some(entry) = selected_bookmark_entry(app) {
        // Archive bookmarks point at an archive file; open it in a new tab as if
        // the user had pressed Enter on it in the file list.
        if entry.is_archive {
            let path = entry.path;
            let Some(filename) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
                app.active_tab_mut().error =
                    Some(format!("'{}' is not a valid archive path", path.display()));
                return;
            };
            crate::handlers::navigation::handle_open_archive(app, &path, filename).await;
            app.popups
                .set_popup_visible(crate::app::PopupKind::Bookmark, false);
            app.popups.reset_popup(crate::app::PopupKind::Bookmark);
            return;
        }

        if entry.is_remote() {
            let user = entry.ssh_user.clone().unwrap_or_else(|| "root".to_string());
            let host = entry.ssh_host.clone().unwrap_or_default();
            let port = entry.ssh_port.unwrap_or(22);

            // Reuse the current connection if this tab is already connected to
            // the same remote, navigating to the bookmarked path instead of
            // opening a new connection/tab.
            let same_connection = matches!(
                app.active_tab().provider.context_key(),
                crate::fs::provider::ContextKey::Ssh {
                    user: ref cu,
                    host: ref ch,
                    port: cp,
                } if cu == &user && ch == &host && cp == port
            );

            if same_connection {
                let selected_path = entry.path;
                match crate::handlers::navigation::navigate_with_fallback(
                    app.active_tab_mut(),
                    &selected_path,
                )
                .await
                {
                    Ok(navigated_path) => {
                        if navigated_path != selected_path {
                            app.active_tab_mut().status_msg = Some((
                                format!(
                                    "'{}' not found, navigated to '{}'",
                                    selected_path.display(),
                                    navigated_path.display()
                                ),
                                Instant::now(),
                            ));
                        }
                    }
                    Err(e) => {
                        app.active_tab_mut().error = Some(format!("Error: {e}"));
                    }
                }
                app.popups
                    .set_popup_visible(crate::app::PopupKind::Bookmark, false);
                app.popups.reset_popup(crate::app::PopupKind::Bookmark);
                return;
            }

            // Different connection: open a new tab. Seed the SSH popup fields so
            // the password-auth fallback (and its resolution of host/port/target)
            // works when pubkey auth fails.
            app.popups.ssh_connection.port = port.to_string();
            let path_str = entry.path.to_string_lossy();
            app.popups.ssh_connection.connection_string = if port == 22 {
                format!("{user}@{host}:{path_str}")
            } else {
                format!("{user}@{host}:{port}:{path_str}")
            };

            // Remember that the password prompt (if any) came from a bookmark so
            // that `Esc` dismisses it instead of reopening the SSH dialog.
            app.popups.ssh_password.from_bookmark = true;

            crate::handlers::popup_ssh::spawn_ssh_connect_with_keys(
                app,
                host,
                port,
                user,
                Some(path_str.to_string()),
                None,
            );

            app.popups
                .set_popup_visible(crate::app::PopupKind::Bookmark, false);
            app.popups.reset_popup(crate::app::PopupKind::Bookmark);
            return;
        }

        // Remote entries return above, so this is always the local branch.
        let selected_path = entry.path;
        navigate_local_bookmark(app, selected_path).await;
    }
    app.popups
        .set_popup_visible(crate::app::PopupKind::Bookmark, false);
    app.popups.reset_popup(crate::app::PopupKind::Bookmark);
}

/// Navigates to a local bookmark, removing it if the path had to fall back.
async fn navigate_local_bookmark(app: &mut AppState, path: PathBuf) {
    match crate::handlers::navigation::navigate_with_fallback(app.active_tab_mut(), &path).await {
        Ok(navigated_path) => {
            if navigated_path != path {
                if let Err(se) = app.bookmark_store.remove_by_path(&path, false) {
                    app.active_tab_mut().error = Some(format!("Failed to save bookmarks: {se}"));
                }
                app.active_tab_mut().status_msg = Some((
                    format!(
                        "'{}' not found, navigated to '{}'",
                        path.display(),
                        navigated_path.display()
                    ),
                    Instant::now(),
                ));
            }
        }
        Err(e) => {
            app.active_tab_mut().error = Some(format!("Error: {e}"));
            if let Err(se) = app.bookmark_store.remove_by_path(&path, false) {
                app.active_tab_mut().error =
                    Some(format!("Error: {e}; also failed to save bookmarks: {se}"));
            }
        }
    }
}
