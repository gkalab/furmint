//! Copy/move popup handler and spawning logic

use crate::app::{AppState, PanelSide};
use crossterm::event::KeyCode;

pub fn handle_init_copy(app: &mut AppState) {
    init_copy_move(app, crate::app::CopyMoveAction::Copy);
}

pub fn handle_init_move(app: &mut AppState) {
    init_copy_move(app, crate::app::CopyMoveAction::Move);
}

pub fn init_copy_move(app: &mut AppState, action: crate::app::CopyMoveAction) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let tab = tab_manager.active_tab();
    let selected: Vec<_> = tab
        .get_selected_entries()
        .iter()
        .map(|e| tab.current_dir.join(&e.name))
        .collect();

    let paths = if selected.is_empty() {
        if let Some(entry) = tab.current_entry() {
            if entry.name == ".." {
                vec![]
            } else {
                vec![tab.current_dir.join(&entry.name)]
            }
        } else {
            vec![]
        }
    } else {
        selected
    };

    if paths.is_empty() {
        return;
    }

    // Get inactive panel path
    let inactive_tab = match app.active {
        PanelSide::Left => app.right.active_tab(),
        PanelSide::Right => app.left.active_tab(),
    };
    let dest = inactive_tab.current_dir.to_string_lossy().to_string();

    app.copy_move_popup.source_paths = paths;
    app.copy_move_popup.action = action;
    app.copy_move_popup.destination_input = dest;
    app.copy_move_popup.cursor_position = app.copy_move_popup.destination_input.len();
    app.copy_move_popup.input_selected = false;
    app.copy_move_popup.is_visible = true;
}

pub fn handle_copy_move_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.copy_move_popup.reset();
        }
        KeyCode::Enter => {
            let dest_input = app.copy_move_popup.destination_input.clone();
            let dest_path = if dest_input.starts_with('~') {
                if let Some(base_dirs) = directories::BaseDirs::new() {
                    let home = base_dirs.home_dir();
                    if dest_input == "~" {
                        home.to_path_buf()
                    } else {
                        home.join(dest_input.trim_start_matches("~/"))
                    }
                } else {
                    std::path::PathBuf::from(dest_input)
                }
            } else {
                std::path::PathBuf::from(dest_input)
            };
            let dest_abs = if let Ok(p) = dest_path.canonicalize() {
                p
            } else if dest_path.is_absolute() {
                dest_path
            } else {
                match app.active {
                    PanelSide::Left => app.left.active_tab().current_dir.join(&dest_path),
                    PanelSide::Right => app.right.active_tab().current_dir.join(&dest_path),
                }
            };
            app.copy_move_popup.destination_input = dest_abs.to_string_lossy().to_string();
            for src in &app.copy_move_popup.source_paths {
                if let Ok(src_abs) = src.canonicalize() {
                    if src_abs == dest_abs {
                        app.copy_move_popup.error =
                            Some("Cannot copy/move source into itself".to_string());
                        return false;
                    }
                    if dest_abs.starts_with(&src_abs) {
                        app.copy_move_popup.error =
                            Some("Cannot copy/move into subdirectory of itself".to_string());
                        return false;
                    }
                    if let Some(file_name) = src_abs.file_name() {
                        let effective_dest = dest_abs.join(file_name);
                        if effective_dest == src_abs {
                            app.copy_move_popup.error =
                                Some("Source and destination are the same".to_string());
                            return false;
                        }
                    }
                }
            }
            spawn_copy_move_task(app);
            app.copy_move_popup.reset();
        }
        KeyCode::Char(c) => {
            app.copy_move_popup.error = None;
            app.copy_move_popup
                .destination_input
                .insert(app.copy_move_popup.cursor_position, c);
            app.copy_move_popup.cursor_position += 1;
        }
        KeyCode::Backspace => {
            if app.copy_move_popup.cursor_position > 0 {
                app.copy_move_popup
                    .destination_input
                    .remove(app.copy_move_popup.cursor_position - 1);
                app.copy_move_popup.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if app.copy_move_popup.cursor_position < app.copy_move_popup.destination_input.len() {
                app.copy_move_popup
                    .destination_input
                    .remove(app.copy_move_popup.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.copy_move_popup.cursor_position > 0 {
                app.copy_move_popup.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.copy_move_popup.cursor_position < app.copy_move_popup.destination_input.len() {
                app.copy_move_popup.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.copy_move_popup.cursor_position = 0;
        }
        KeyCode::End => {
            app.copy_move_popup.cursor_position = app.copy_move_popup.destination_input.len();
        }
        _ => {}
    }
    false
}

pub fn spawn_copy_move_task(app: &mut AppState) {
    let paths = app.copy_move_popup.source_paths.clone();
    let dest_str = app.copy_move_popup.destination_input.clone();
    let action = app.copy_move_popup.action;

    // Validate destination
    let dest_path = std::path::PathBuf::from(&dest_str);

    let task_name = match action {
        crate::app::CopyMoveAction::Copy => format!("Copying {} items", paths.len()),
        crate::app::CopyMoveAction::Move => format!("Moving {} items", paths.len()),
    };

    // Deselect files in active panel
    {
        let entries = match app.active {
            crate::app::PanelSide::Left => &mut app.left.active_tab_mut().entries,
            crate::app::PanelSide::Right => &mut app.right.active_tab_mut().entries,
        };
        for entry in entries.iter_mut() {
            if entry.selected {
                entry.selected = false;
            }
        }
    }

    // Create channel for decisions
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(1);

    let id = app
        .task_manager
        .spawn_task(task_name, move |cancel, tx, id| async move {
            // Pre-calculation of total items (approximate)
            let total_items = crate::handlers::file_ops::count_items(&paths);
            let processed_items = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

            // State for "Apply to all" decisions
            // We use a struct to hold this state across recursions
            let mut decision_state = crate::handlers::file_ops::DecisionState {
                overwrite_all: false,
                skip_all: false,

                last_update: std::time::Instant::now(),
            };

            // We need `decision_rx` to be mutual, so we wrap it
            let decision_rx = std::sync::Arc::new(tokio::sync::Mutex::new(decision_rx));

            // Ensure dest dir exists if multiple items or if treated as dir
            let treat_as_dir = paths.len() > 1
                || dest_path.is_dir()
                || dest_str.ends_with(std::path::MAIN_SEPARATOR);

            if treat_as_dir {
                if let Err(e) = tokio::fs::create_dir_all(&dest_path).await {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Failed(e.to_string()),
                    ));
                    return;
                }
            } else if let Some(parent) = dest_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            let mut failures = Vec::new();

            for src in &paths {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Cancelled,
                    ));
                    return;
                }

                let file_name = match src.file_name() {
                    Some(n) => n,
                    None => continue,
                };

                let target = if treat_as_dir {
                    dest_path.join(file_name)
                } else {
                    dest_path.clone()
                };

                // Recursive copy/move
                let ctx = crate::handlers::file_ops::RecursiveOpContext {
                    tx: &tx,
                    id,
                    total: total_items,
                    processed: &processed_items,
                    decision_rx: &decision_rx,
                };
                let fs_impl = crate::handlers::file_ops::StdFileSystem;
                let res = crate::handlers::file_ops::recursive_op(
                    &fs_impl,
                    src,
                    &target,
                    action,
                    &cancel,
                    ctx,
                    &mut decision_state,
                )
                .await;

                if let Err(e) = res {
                    failures.push(e);
                }
            }

            if failures.is_empty() {
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Completed,
                ));
            } else {
                // ... error handling
                let msg = format!("Failed with {} errors", failures.len());
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Failed(msg),
                ));
            }
        });

    // Store decision tx
    app.task_decision_txs.insert(id, decision_tx);
}
