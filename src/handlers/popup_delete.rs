//! Delete popup event handler and deletion logic

use crate::app::AppState;
use crossterm::event::KeyCode;

pub(crate) fn handle_delete_popup_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc | KeyCode::Char('n') => {
            app.delete_popup.reset();
        }
        KeyCode::Char('y') | KeyCode::Enter => {
            handle_confirm_delete(app);
            app.delete_popup.reset();
        }
        _ => {}
    }
    false
}

pub(crate) fn handle_init_delete(app: &mut AppState, permanent: bool) {
    let tab_manager = match app.active {
        crate::app::PanelSide::Left => &mut app.left,
        crate::app::PanelSide::Right => &mut app.right,
    };
    let tab = tab_manager.active_tab();
    let mut selected: Vec<_> = tab
        .get_selected_entries()
        .iter()
        .map(|e| tab.current_dir.join(&e.name))
        .collect();

    if selected.is_empty()
        && let Some(entry) = tab.current_entry()
        && entry.name != ".."
    {
        selected.push(tab.current_dir.join(&entry.name));
    }

    if selected.is_empty() {
        return;
    }

    app.delete_popup.selected_paths = selected;
    app.delete_popup.is_permanent = permanent;
    app.delete_popup.is_visible = true;
}

pub(crate) fn handle_confirm_delete(app: &mut AppState) {
    let paths = app.delete_popup.selected_paths.clone();
    let is_permanent = app.delete_popup.is_permanent;

    let name = if is_permanent {
        format!("Deleting {} items permanently", paths.len())
    } else {
        format!("Trashing {} items", paths.len())
    };

    app.task_manager
        .spawn_task(name, move |cancel, tx, id| async move {
            let total = paths.len();
            let mut success = 0;
            let mut failures = Vec::new();
            for (i, path) in paths.iter().enumerate() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Cancelled,
                    ));
                    return;
                }
                let result = if is_permanent {
                    if path.is_dir() {
                        std::fs::remove_dir_all(path)
                    } else {
                        std::fs::remove_file(path)
                    }
                    .map_err(|e| e.to_string())
                } else {
                    trash::delete(path).map_err(|e| e.to_string())
                };
                match result {
                    Ok(()) => success += 1,
                    Err(e) => failures.push(format!("{}: {}", path.display(), e)),
                }
                let _ = tx.send(crate::tasks::TaskEvent::UpdateProgress(id, i + 1, total));
            }
            if failures.is_empty() {
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Completed,
                ));
            } else {
                let error_msg = if success > 0 {
                    format!("Completed with errors: {} failed", failures.len())
                } else {
                    format!("Failed: {}", failures[0])
                };
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Failed(error_msg),
                ));
            }
        });

    // Clear selection in active tab if deletion started
    let tab_manager = match app.active {
        crate::app::PanelSide::Left => &mut app.left,
        crate::app::PanelSide::Right => &mut app.right,
    };
    for entry in &mut tab_manager.active_tab_mut().entries {
        entry.selected = false;
    }
}
