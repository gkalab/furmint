//! Navigation-related event handlers for directory and panel navigation.

use crate::app::{AppState, PanelSide};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

// Moves the cursor up in the active panel.
pub fn handle_up(app: &mut AppState) {
    app.active_tab_mut().move_cursor_up();
    update_viewer_content(app);
}

// Moves the cursor down in the active panel.
pub fn handle_down(app: &mut AppState) {
    app.active_tab_mut().move_cursor_down();
    update_viewer_content(app);
}

// Moves the cursor a page up.
pub fn handle_page_up(app: &mut AppState) {
    app.active_tab_mut().move_cursor_page_up(20);
    update_viewer_content(app);
}

// Moves the cursor a page down.
pub fn handle_page_down(app: &mut AppState) {
    app.active_tab_mut().move_cursor_page_down(20);
    update_viewer_content(app);
}

// Moves the cursor to the home position.
pub fn handle_home(app: &mut AppState) {
    app.active_tab_mut().move_cursor_home();
    update_viewer_content(app);
}

// Moves the cursor to the end.
pub fn handle_end(app: &mut AppState) {
    app.active_tab_mut().move_cursor_end();
    update_viewer_content(app);
}

// Handles quick type-to-select in the active panel.
pub fn handle_type_char(app: &mut AppState, c: char) {
    use std::time::Instant;
    let panel = app.active_tab_mut();
    let now = Instant::now();
    let reset_threshold = std::time::Duration::from_secs(1);
    // If last_type_time is None or too old, reset buffer
    if panel
        .last_type_time
        .is_none_or(|t| now.duration_since(t) > reset_threshold)
    {
        panel.typed_buffer.clear();
    }
    panel.typed_buffer.push(c);
    panel.last_type_time = Some(now);
    let typed = panel.typed_buffer.to_lowercase();
    panel.search_highlights.clear();

    // Find all entries whose name starts with typed
    panel.matching_indices = panel
        .entries
        .iter()
        .enumerate()
        .filter(|(idx, entry)| {
            if entry.name.to_lowercase().starts_with(&typed) {
                panel
                    .search_highlights
                    .insert(*idx, (0..typed.len()).collect());
                true
            } else {
                false
            }
        })
        .map(|(idx, _)| idx)
        .collect();

    // If no prefix matches, try fuzzy matching
    if panel.matching_indices.is_empty() {
        let matcher = SkimMatcherV2::default();
        let mut results: Vec<(usize, i64, Vec<usize>)> = panel
            .entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                matcher
                    .fuzzy_indices(&entry.name.to_lowercase(), &typed)
                    .map(|(score, indices)| (idx, score, indices))
            })
            .collect();

        // Sort by score (descending)
        results.sort_by(|a, b| b.1.cmp(&a.1));

        for (idx, _, indices) in &results {
            panel.search_highlights.insert(*idx, indices.clone());
        }
        panel.matching_indices = results.into_iter().map(|(idx, _, _)| idx).collect();
    }

    // Select first match
    if let Some(&idx) = panel.matching_indices.first() {
        panel.cursor = idx;
        panel.search_position = 0;
    }
    update_viewer_content(app);
}

/// Handle Up arrow during active search - move to previous match (wraps)
pub fn handle_up_search(app: &mut AppState) {
    let panel = app.active_tab_mut();
    // Restart timer
    panel.last_type_time = Some(std::time::Instant::now());
    // If only one match or no matches, do nothing
    if panel.matching_indices.len() <= 1 {
        return;
    }
    // Decrement position with wrap-around
    if panel.search_position == 0 {
        panel.search_position = panel.matching_indices.len() - 1;
    } else {
        panel.search_position -= 1;
    }
    // Move cursor to the matched index
    panel.cursor = panel.matching_indices[panel.search_position];
    update_viewer_content(app);
}

/// Handle Down arrow during active search - move to next match (wraps)
pub fn handle_down_search(app: &mut AppState) {
    let panel = app.active_tab_mut();
    // Restart timer
    panel.last_type_time = Some(std::time::Instant::now());
    // If only one match or no matches, do nothing
    if panel.matching_indices.len() <= 1 {
        return;
    }
    // Increment position with wrap-around
    panel.search_position = (panel.search_position + 1) % panel.matching_indices.len();
    // Move cursor to the matched index
    panel.cursor = panel.matching_indices[panel.search_position];
    update_viewer_content(app);
}

/// Reset search state (called on Esc or timeout)
pub fn reset_search(app: &mut AppState) {
    let panel = app.active_tab_mut();
    panel.typed_buffer.clear();
    panel.matching_indices.clear();
    panel.search_highlights.clear();
    panel.search_position = 0;
    panel.last_type_time = None;
}

/// Reset search state if timeout has expired (called periodically and for navigation keys)
pub fn reset_expired_search(app: &mut AppState) {
    let panel = app.active_tab_mut();
    if !panel.typed_buffer.is_empty()
        && panel.last_type_time.is_some_and(|t| {
            std::time::Instant::now().duration_since(t) > std::time::Duration::from_secs(1)
        })
    {
        panel.typed_buffer.clear();
        panel.matching_indices.clear();
        panel.search_highlights.clear();
        panel.search_position = 0;
        panel.last_type_time = None;
    }
}

// Switches between panels or focuses file viewer.
pub fn handle_tab(app: &mut AppState) {
    if app.file_viewer.is_visible {
        app.file_viewer.focused = true;
    } else {
        app.active = match app.active {
            PanelSide::Left => PanelSide::Right,
            PanelSide::Right => PanelSide::Left,
        };
    }
}

/// Update content in external file viewer based on cursor.
pub fn update_viewer_content(app: &mut AppState) {
    if !app.file_viewer.is_visible {
        return;
    }
    let panel = app.active_tab();
    let Some(entry) = panel.current_entry() else {
        app.file_viewer.content = vec![];
        return;
    };
    if entry.is_dir {
        app.file_viewer.content = vec!["Directory".to_string()];
    } else {
        let full_path = panel.current_dir.join(&entry.name);
        let provider = panel.provider.clone();
        app.file_viewer.load_content(full_path, provider);
    }
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
                    vec![full_path.to_string_lossy().to_string()],
                    false,
                ) {
                    app.active_tab_mut().error = Some(format!("Error launching in terminal: {e}"));
                }
            } else {
                #[cfg(target_os = "linux")]
                {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&full_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
                #[cfg(not(target_os = "linux"))]
                {
                    if let Err(e) = open::that(&full_path) {
                        app.active_tab_mut().error = Some(format!("Error opening file: {}", e));
                    }
                }
            }
        }
    }
}

pub fn handle_directory_up(app: &mut AppState) {
    let parent_path = app
        .active_tab()
        .current_dir
        .parent()
        .map(|p| p.to_path_buf());

    let context_key = app.active_tab().provider.context_key();

    if let Some(path) = parent_path {
        if let Err(e) = app.active_tab_mut().go_up() {
            app.active_tab_mut().error = Some(format!("Error: {e}"));
        } else {
            app.dir_history.record_visit(&context_key, &path);
            update_viewer_content(app);
        }
    }
}

pub fn handle_history_previous(app: &mut AppState) {
    // Get context key before mutating mostly to be safe/consistent
    let context_key = app.active_tab().provider.context_key();

    if let Err(e) = app.active_tab_mut().go_back() {
        app.active_tab_mut().error = Some(format!("Error: {e}"));
    } else {
        let current_dir = app.active_tab().current_dir.clone();
        app.dir_history.record_visit(&context_key, &current_dir);
        update_viewer_content(app);
    }
}

pub fn handle_history_next(app: &mut AppState) {
    let context_key = app.active_tab().provider.context_key();

    if let Err(e) = app.active_tab_mut().go_forward() {
        app.active_tab_mut().error = Some(format!("Error: {e}"));
    } else {
        let current_dir = app.active_tab().current_dir.clone();
        app.dir_history.record_visit(&context_key, &current_dir);
        update_viewer_content(app);
    }
}

pub fn handle_sort(app: &mut AppState, column: crate::app::SortColumn) {
    let (col, dir, context_key) = {
        let tab = app.active_tab_mut();
        tab.handle_sort(column);
        (
            tab.sort_column,
            tab.sort_direction,
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
