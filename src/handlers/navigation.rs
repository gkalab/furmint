//! Navigation-related event handlers for directory and panel navigation.

use crate::app::{AppState, SortColumn};
use crate::app_state::tabs::Tab;
use crate::fs::fs_archive::ArchiveFs;
use ratatui::layout::Rect;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// Moves the cursor up in the active panel.
pub fn handle_up(app: &mut AppState) {
    app.active_tab_mut().move_cursor_up_filtered();
    update_viewer_content(app);
}

// Moves the cursor down in the active panel.
pub fn handle_down(app: &mut AppState) {
    app.active_tab_mut().move_cursor_down_filtered();
    update_viewer_content(app);
}

// Moves the cursor a page up.
pub fn handle_page_up(app: &mut AppState) {
    let tab = app.active_tab_mut();
    if tab.has_file_filter() {
        let page = 20.min(tab.visible_count().saturating_sub(1));
        tab.move_cursor_page_up_filtered(page);
    } else {
        tab.move_cursor_page_up(20);
    }
    update_viewer_content(app);
}

// Moves the cursor a page down.
pub fn handle_page_down(app: &mut AppState) {
    let tab = app.active_tab_mut();
    if tab.has_file_filter() {
        let page = 20.min(tab.visible_count().saturating_sub(1));
        tab.move_cursor_page_down_filtered(page);
    } else {
        tab.move_cursor_page_down(20);
    }
    update_viewer_content(app);
}

// Enters the selected directory or opens the file.
pub fn handle_enter(app: &mut AppState) {
    if let Some((path, _, filename)) = archive_path_and_ext(app) {
        handle_open_archive(app, &path, filename);
        return;
    }

    handle_open_item(app);
}

fn archive_path_and_ext(app: &mut AppState) -> Option<(PathBuf, String, String)> {
    // Check if we are selecting a file that is a supported archive
    let panel = app.active_tab();
    if let Some(entry) = panel.current_entry() {
        if entry.is_dir {
            None
        } else {
            let path = panel.current_dir.join(&entry.name);
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext = ext.to_lowercase();
                if [
                    "zip", "jar", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "rpm",
                ]
                .contains(&ext.as_str())
                {
                    Some((path, ext, entry.name.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        }
    } else {
        None
    }
}

// Moves up to the parent directory.
pub fn handle_left(app: &mut AppState) {
    let panel = app.active_tab_mut();
    // If we are at the root of an archive, we might want to exit it?
    // Current go_up implementation handles ".." logic typically.
    // But if we are at "/" of an archive, go_up might do nothing or we might want to "leave" the provider.
    // However, the standard `enter_dir` on ".." handles going up.
    // If we want Left Arrow to go to parent:
    if let Err(e) = panel.go_up() {
        panel.error = Some(e.to_string());
    }
    update_viewer_content(app);
}

// Enters the selected directory (same as Enter for now).
pub fn handle_right(app: &mut AppState) {
    handle_enter(app);
}

// Moves the cursor to the home position.
pub fn handle_home(app: &mut AppState) {
    app.active_tab_mut().move_cursor_home_filtered();
    update_viewer_content(app);
}

// Moves the cursor to the end.
pub fn handle_end(app: &mut AppState) {
    app.active_tab_mut().move_cursor_end_filtered();
    update_viewer_content(app);
}

// Handles quick type-to-select in the active panel.
pub fn handle_type_char(app: &mut AppState, c: char) {
    let panel = app.active_tab_mut();

    // If search is not active (timed out or not started), clear buffer
    if !panel.is_search_active() {
        panel.search.buffer.clear();
    }

    panel.search.buffer.push(c);
    panel.search.last_type_time = Some(std::time::Instant::now());

    panel.apply_search_highlights();

    // Select first match
    if let Some(&idx) = panel.search.matching_indices.first() {
        panel.cursor = idx;
        panel.search.position = 0;
    }
    update_viewer_content(app);
}

/// Handle Up arrow during active search - move to previous match (wraps)
pub fn handle_up_search(app: &mut AppState) {
    let panel = app.active_tab_mut();
    // Restart timer
    panel.search.last_type_time = Some(std::time::Instant::now());
    // If only one match or no matches, do nothing
    if panel.search.matching_indices.len() <= 1 {
        return;
    }
    // Decrement position with wrap-around
    if panel.search.position == 0 {
        panel.search.position = panel.search.matching_indices.len() - 1;
    } else {
        panel.search.position -= 1;
    }
    // Move cursor to the matched index
    panel.cursor = panel.search.matching_indices[panel.search.position];
    update_viewer_content(app);
}

/// Handle Down arrow during active search - move to next match (wraps)
pub fn handle_down_search(app: &mut AppState) {
    let panel = app.active_tab_mut();
    // Restart timer
    panel.search.last_type_time = Some(std::time::Instant::now());
    // If only one match or no matches, do nothing
    if panel.search.matching_indices.len() <= 1 {
        return;
    }
    // Increment position with wrap-around
    panel.search.position = (panel.search.position + 1) % panel.search.matching_indices.len();
    // Move cursor to the matched index
    panel.cursor = panel.search.matching_indices[panel.search.position];
    update_viewer_content(app);
}

/// Reset search state (called on Esc or timeout)
pub fn reset_search(app: &mut AppState) {
    app.active_tab_mut().reset_search();
}

/// Reset search state if timeout has expired (called periodically and for navigation keys)
pub fn reset_expired_search(app: &mut AppState) {
    let handle_tabs = |tabs: &mut [crate::app::Tab]| {
        for tab in tabs {
            if !tab.is_search_active() {
                tab.reset_search();
            }
        }
    };

    handle_tabs(&mut app.left.tabs);
    handle_tabs(&mut app.right.tabs);
}

// Switches between panels or focuses file viewer.
pub fn handle_tab(app: &mut AppState) {
    if app.file_viewer.is_visible {
        app.file_viewer.focused = true;
    } else {
        app.toggle_active_panel();
    }
}

/// Update content in external file viewer based on cursor.
pub fn update_viewer_content(app: &mut AppState) {
    if !app.file_viewer.is_visible {
        return;
    }
    let panel = app.active_tab();
    let Some(entry) = panel.current_entry() else {
        app.file_viewer.reset();
        return;
    };
    let full_path = panel.current_dir.join(&entry.name);
    let provider = panel.provider.clone();
    let size = entry.size;
    let max_file_size = 10 * 1024 * 1024;
    app.file_viewer
        .load_content(&full_path, &provider, size, max_file_size);
}

pub fn handle_enter_directory(app: &mut AppState) {
    let entry_opt = app.active_tab().current_entry().cloned();

    if let Some(entry) = entry_opt
        && entry.is_dir
    {
        if entry.name == ".." {
            let context_key = app.active_tab().provider.context_key();
            let current_dir = app.active_tab().current_dir.clone();

            if let Err(e) = app.active_tab_mut().go_up() {
                app.active_tab_mut().error = Some(format!("Error: {e}"));
            } else {
                app.dir_history.record_visit(&context_key, &current_dir);
                update_viewer_content(app);
            }
        } else {
            let path = app.active_tab().current_dir.join(&entry.name);
            if let Err(e) = app.active_tab_mut().navigate_to(&path) {
                app.active_tab_mut().error = Some(format!("Error: {e}"));
            } else {
                let context_key = app.active_tab().provider.context_key();
                app.dir_history.record_visit(&context_key, &path);
                update_viewer_content(app);
            }
        }
    }
}

fn handle_open_archive(app: &mut AppState, path: &PathBuf, filename: String) {
    let panel = app.active_tab_mut();
    if !panel.provider.is_local() {
        panel.error = Some("Opening archives from remote connections is not supported".to_string());
        update_viewer_content(app);
        return;
    }

    let side_index = match app.active {
        crate::app::PanelSide::Left => 0,
        crate::app::PanelSide::Right => 1,
    };

    // Check cache first
    if let Ok(metadata) = std::fs::metadata(path)
        && let Some(entry) = app.archive_cache.get(path)
    {
        let current_mtime = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let current_size = metadata.len();

        if entry.mtime == current_mtime && entry.size == current_size {
            let provider = entry.provider.clone();
            // Create tab immediately
            let manager = if side_index == 0 {
                &mut app.left
            } else {
                &mut app.right
            };

            match crate::app::Tab::with_provider(&std::path::PathBuf::from("/"), provider) {
                Ok(mut tab) => {
                    tab.custom_title = Some(filename);
                    manager.tabs.push(tab);
                    manager.active_tab_index = manager.tabs.len() - 1;
                    // Set active panel
                    app.active = if side_index == 0 {
                        crate::app::PanelSide::Left
                    } else {
                        crate::app::PanelSide::Right
                    };
                }
                Err(e) => {
                    manager.active_tab_mut().error =
                        Some(format!("Failed to create archive tab from cache: {e}"));
                }
            }
            return;
        }
        // Cache invalid
        app.archive_cache.remove(path);
    }

    let path_clone = path.clone();
    let filename_clone = filename.clone();
    let path_for_event = path.clone();

    let task_name = format!("Opening {filename}");

    app.task_manager
        .spawn_task(&task_name, move |_cancel, tx, id| async move {
            let path_for_task = path_clone.clone();
            // We need to run blocking IO
            let res = tokio::task::spawn_blocking(move || ArchiveFs::new(&path_for_task)).await;

            match res {
                Ok(Ok(archive_fs)) => {
                    let provider = Arc::new(archive_fs);
                    let wrapper = crate::tasks::ProviderWrapper(provider);
                    let _ = tx.send(crate::tasks::TaskEvent::ArchiveLoaded(
                        side_index,
                        wrapper,
                        filename_clone,
                        path_for_event,
                    ));
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Completed,
                    ));
                }
                Ok(Err(e)) => {
                    let _ = tx.send(crate::tasks::TaskEvent::Error(
                        id,
                        path_clone.to_string_lossy().to_string(),
                        e.to_string(),
                    ));
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Failed(e.to_string()),
                    ));
                }
                Err(e) => {
                    // Join error
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Failed(e.to_string()),
                    ));
                }
            }
        });
}

pub fn handle_open_item(app: &mut AppState) {
    let entry_opt = app.active_tab().current_entry().cloned();

    if let Some(entry) = entry_opt {
        if entry.is_dir {
            handle_enter_directory(app);
        } else if app.active_tab().provider.is_local() {
            let (full_path, current_dir) = {
                let panel = app.active_tab();
                (
                    panel.current_dir.join(&entry.name),
                    panel.current_dir.clone(),
                )
            };
            let is_exe = crate::fs::utils::is_executable(&full_path, &entry);

            if is_exe {
                let configured_terminal = app.global.terminal.clone();
                if let Err(e) = crate::handlers::terminal::spawn_terminal(
                    &current_dir,
                    configured_terminal,
                    &[full_path.to_string_lossy().to_string()],
                    false,
                ) {
                    app.active_tab_mut().error = Some(format!("Error launching in terminal: {e}"));
                }
            } else if let Err(e) = app.opener.open(&full_path) {
                app.active_tab_mut().error = Some(format!("Error opening file: {e}"));
            }
        }
    }
}

pub fn handle_directory_up(app: &mut AppState) {
    let parent_path = app
        .active_tab()
        .current_dir
        .parent()
        .map(std::path::Path::to_path_buf);

    let context_key = app.active_tab().provider.context_key();

    if let Some(path) = parent_path {
        if let Err(e) = app.active_tab_mut().go_up() {
            app.active_tab_mut().error = Some(format!("Error: {e}"));
        } else {
            if !context_key.starts_with("archive:") {
                app.dir_history.record_visit(&context_key, &path);
            }
            update_viewer_content(app);
        }
    }
}

/// Map a mouse click on a panel's column header to a sort action.
/// `area` is the panel's outer area (including borders) and `borders`
/// indicates whether the panel has visible borders.
pub fn handle_header_click(app: &mut AppState, x: u16, area: Rect, borders: bool) {
    let border_offset = u16::from(borders);
    let inner_x = area.x + border_offset;
    let inner_width = area.width.saturating_sub(border_offset * 2);

    // Column widths matching table constraints at panel.rs:
    // [Min(10), Length(7), Length(19), Length(ATTRIBUTES_COL_WIDTH)]
    let attributes_width = crate::ui::panel::ATTRIBUTES_COL_WIDTH;
    let name_width = inner_width.saturating_sub(7 + 19 + attributes_width);
    let size_x = inner_x + name_width;
    let modified_x = size_x + 7;
    let attributes_x = modified_x + 19;

    let column = if x < size_x {
        SortColumn::Name
    } else if x < modified_x {
        SortColumn::Size
    } else if x < attributes_x {
        SortColumn::Date
    } else {
        return;
    };

    handle_sort(app, column);
}

pub fn handle_sort(app: &mut AppState, column: crate::app::SortColumn) {
    let (col, dir, context_key) = {
        let tab = app.active_tab_mut();
        tab.handle_sort(column);
        (
            tab.sort.column,
            tab.sort.direction,
            tab.provider.context_key(),
        )
    };

    if context_key.starts_with('[') && context_key.ends_with(']') {
        let inner = &context_key[1..context_key.len() - 1];
        if let Some(at_idx) = inner.find('@') {
            let user = &inner[..at_idx];
            let host = &inner[at_idx + 1..];
            let name = {
                let tab = app.active_tab();
                tab.custom_title.clone()
            };
            app.ssh_history
                .update_sort_settings(host, user, name.as_deref(), col, dir);
        }
    }

    update_viewer_content(app);
}

pub fn handle_toggle_selection(app: &mut AppState) {
    app.active_tab_mut().toggle_selection();
    update_viewer_content(app);
}

/// Navigate to `target`, walking up ancestors if it no longer exists.
///
/// Returns the path that was successfully navigated to.
///
/// # Errors
///
/// Returns the original error if neither the target nor any ancestor can be listed.
pub fn navigate_with_fallback(tab: &mut Tab, target: &Path) -> Result<PathBuf, anyhow::Error> {
    // Try the target path first
    let original_error = match tab.navigate_to(target) {
        Ok(()) => return Ok(target.to_path_buf()),
        Err(e) => e,
    };

    // Walk up ancestors looking for an existing directory
    let mut current = target.to_path_buf();
    while let Some(parent) = current.parent() {
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
        if tab.navigate_to(current.as_path()).is_ok() {
            return Ok(current);
        }
    }

    Err(original_error)
}
