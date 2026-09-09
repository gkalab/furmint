//! Delete popup event handler and deletion logic

use crate::app::AppState;
use termina::event::KeyCode;

pub fn handle_delete_event(code: KeyCode, app: &mut AppState) -> bool {
    use crate::handlers::popup_utils::{ChoiceResult, get_choice_with_selection};
    match get_choice_with_selection(code, &mut app.popups.delete.selected_no) {
        ChoiceResult::Confirmed => {
            handle_confirm_delete(app);
            app.popups.reset_popup(crate::app::PopupKind::Delete);
        }
        ChoiceResult::Cancelled => {
            app.popups.reset_popup(crate::app::PopupKind::Delete);
        }
        ChoiceResult::None => {}
    }
    false
}

pub fn handle_init_delete(app: &mut AppState, permanent: bool) {
    let tab = app.active_tab();
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

    let is_remote = !tab.provider.context_key().is_local();

    app.popups.delete.selected_paths = selected;
    app.popups.delete.is_permanent = permanent || is_remote;
    app.popups
        .set_popup_visible(crate::app::PopupKind::Delete, true);
}

pub fn handle_confirm_delete(app: &mut AppState) {
    let paths = app.popups.delete.selected_paths.clone();
    let is_permanent = app.popups.delete.is_permanent;
    let provider = app.active_tab().provider.clone();

    let name = if is_permanent {
        format!("Deleting {} items permanently", paths.len())
    } else {
        format!("Trashing {} items", paths.len())
    };

    app.tasks
        .task_manager
        .spawn_task(&name, move |cancel, tx, id| async move {
            let total = paths.len();
            let mut success = 0;
            let mut failures = Vec::new();
            for (i, path) in paths.iter().enumerate() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = tx.send(crate::tasks::UiEvent::Task(
                        crate::tasks::TaskEvent::UpdateStatus {
                            task_id: id,
                            status: crate::tasks::TaskStatus::Cancelled,
                        },
                    ));
                    return;
                }

                let result = if is_permanent {
                    provider.delete(path, true).await.map_err(|e| e.to_string())
                } else {
                    let path_buf = path.clone();
                    tokio::task::spawn_blocking(move || {
                        trash::delete(&path_buf).map_err(|e| anyhow::anyhow!(e))
                    })
                    .await
                    .unwrap_or_else(|e| Err(anyhow::anyhow!("Task join error: {e}")))
                    .map_err(|e| e.to_string())
                };

                match result {
                    Ok(()) => success += 1,
                    Err(e) => failures.push(format!("{}: {e}", path.display())),
                }
                let _ = tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateProgress {
                        task_id: id,
                        processed: i + 1,
                        total,
                    },
                ));
            }
            if failures.is_empty() {
                let _ = tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateStatus {
                        task_id: id,
                        status: crate::tasks::TaskStatus::Completed,
                    },
                ));
            } else {
                let error_msg = if success > 0 {
                    format!("Completed with errors: {} failed", failures.len())
                } else {
                    format!("Failed: {}", failures[0])
                };
                let _ = tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateStatus {
                        task_id: id,
                        status: crate::tasks::TaskStatus::Failed(error_msg),
                    },
                ));
            }
        });

    // Clear selection in active tab if deletion started
    for entry in &mut app.active_tab_mut().entries {
        entry.selected = false;
    }
}
