//! Copy/move popup handler and spawning logic

use crate::app::AppState;
use crate::clipboard::FileClipboardData;
use crate::fs::provider::FileSystemProvider;
use crate::state::CopyMoveAction;
use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use termina::event::{KeyCode, Modifiers};

fn get_paths_to_act_on(app: &AppState) -> Vec<PathBuf> {
    let tab = app.active_tab();
    let current_dir = &tab.current_dir;
    let is_local = tab.provider.is_local();

    let selected_entries = tab.get_selected_entries();
    let entries = if selected_entries.is_empty() {
        // Fall back to the entry under the cursor. The cursor always sits on a
        // visible row, so anything under it is on screen.
        match tab.current_entry() {
            Some(entry) if entry.name != ".." => vec![entry],
            _ => vec![],
        }
    } else {
        selected_entries
    };

    entries
        .into_iter()
        .map(|e| {
            if is_local {
                current_dir.join(&e.name)
            } else {
                let s = current_dir.to_string_lossy().to_string();
                let mut s = s.replace('\\', "/");
                if !s.ends_with('/') {
                    s.push('/');
                }
                s.push_str(&e.name);
                PathBuf::from(s)
            }
        })
        .collect()
}

pub fn handle_init_copy(app: &mut AppState) {
    init_copy_move(app, crate::state::CopyMoveAction::Copy);
}

pub fn handle_init_move(app: &mut AppState) {
    init_copy_move(app, crate::state::CopyMoveAction::Move);
}

pub fn init_copy_move(app: &mut AppState, action: crate::state::CopyMoveAction) {
    let paths = get_paths_to_act_on(app);

    if paths.is_empty() {
        return;
    }

    // Get inactive panel path
    let inactive_tab = app.inactive_tab();
    let dest = inactive_tab.current_dir.to_string_lossy().to_string();

    app.popups.copy_move.show(paths, action, dest);
}

pub fn handle_clipboard_copy(app: &mut AppState) {
    handle_clipboard_action(app, crate::clipboard::FileClipboardAction::Copy);
}

pub fn handle_clipboard_cut(app: &mut AppState) {
    handle_clipboard_action(app, crate::clipboard::FileClipboardAction::Cut);
}

fn handle_clipboard_action(app: &mut AppState, action: crate::clipboard::FileClipboardAction) {
    let paths = get_paths_to_act_on(app);

    if paths.is_empty() {
        return;
    }

    let count = paths.len();
    let action_str = match action {
        crate::clipboard::FileClipboardAction::Copy => "copied",
        crate::clipboard::FileClipboardAction::Cut => "cut",
    };
    app.active_tab_mut().status_msg = Some((
        format!(
            "{} item{} {}",
            count,
            if count == 1 { "" } else { "s" },
            action_str
        ),
        std::time::Instant::now(),
    ));

    let data = FileClipboardData {
        action,
        paths,
        source_provider: app.active_tab().provider.clone(),
    };

    let _ = app.os.clipboard.set(data);
}

pub async fn handle_paste(app: &mut AppState) {
    if let Ok(Some(data)) = app.os.clipboard.get() {
        let action = match data.action {
            crate::clipboard::FileClipboardAction::Copy => CopyMoveAction::Copy,
            crate::clipboard::FileClipboardAction::Cut => CopyMoveAction::Move,
        };

        let dest_provider = app.active_tab().provider.clone();
        let dest_path = app.active_tab().current_dir.clone();

        if let Err(err) = validate_copy_move(
            &data.paths,
            &data.source_provider,
            &dest_path,
            &dest_provider,
        )
        .await
        {
            app.active_tab_mut().error = Some(err.to_string());
            return;
        }

        let dest_str = dest_path.to_string_lossy().to_string();
        spawn_copy_move_task(
            app,
            data.source_provider,
            dest_provider,
            data.paths.clone(),
            dest_str,
            action,
        );

        if data.action == crate::clipboard::FileClipboardAction::Cut {
            let _ = app.os.clipboard.clear();
        }
    }
}

/// Returns `true` if `path` already points at an absolute location of the
/// destination provider.
///
/// Local destinations follow the rules of the host OS, so `Path::is_absolute`
/// is correct there. Archives and remote hosts always use Unix-style paths
/// (`/dir/file`), which `is_absolute` rejects on Windows because they carry no
/// drive prefix — there, anything rooted counts as absolute.
#[must_use]
fn is_absolute_dest(path: &Path, dest_is_local: bool) -> bool {
    if dest_is_local {
        path.is_absolute()
    } else {
        path.has_root()
    }
}

/// Appends `dest` to `base`, keeping the POSIX separators used by archives and
/// remote hosts. `Path::join` would insert a `\` on Windows, and joining a
/// rooted `dest` would not replace `base` there, either.
fn join_dest(base: &Path, dest: &Path, dest_is_local: bool) -> PathBuf {
    if dest_is_local {
        return base.join(dest);
    }

    let base_str = crate::fs::utils::normalize_sftp_path(base);
    let dest_str = dest.to_string_lossy().replace('\\', "/");
    let joined = if dest_str.starts_with('/') {
        dest_str
    } else {
        format!("{base_str}/{dest_str}")
    };
    PathBuf::from(crate::fs::utils::normalize_sftp_path(Path::new(&joined)))
}

/// Validates that source paths are not being copied/moved into themselves or subdirectories of themselves.
/// Returns `Err(error_message)` if validation fails, Ok(()) otherwise.
async fn validate_copy_move(
    src_paths: &[PathBuf],
    src_provider: &Arc<dyn FileSystemProvider>,
    dest_path: &std::path::Path,
    dest_provider: &Arc<dyn FileSystemProvider>,
) -> Result<()> {
    if src_provider.context_key() != dest_provider.context_key() {
        return Ok(());
    }

    let dest_abs = if let Ok(p) = dest_provider.canonicalize(dest_path).await {
        p
    } else if dest_path.is_absolute() {
        dest_path.to_path_buf()
    } else {
        // Fallback to raw path if we can't do better
        dest_path.to_path_buf()
    };

    for src in src_paths {
        if let Ok(src_abs) = src_provider.canonicalize(src).await {
            let s_src = src_abs.to_string_lossy();
            let s_dest = dest_abs.to_string_lossy();

            #[cfg(windows)]
            let (n_src, n_dest) = (
                s_src.to_lowercase().replace('/', "\\"),
                s_dest.to_lowercase().replace('/', "\\"),
            );
            #[cfg(not(windows))]
            let (n_src, n_dest) = (s_src.to_string(), s_dest.to_string());

            #[cfg(windows)]
            let (n_src_norm, n_dest_norm) = (
                n_src.strip_prefix(r"\\?\").unwrap_or(&n_src).to_string(),
                n_dest.strip_prefix(r"\\?\").unwrap_or(&n_dest).to_string(),
            );
            #[cfg(not(windows))]
            let (n_src_norm, n_dest_norm) = (n_src, n_dest);

            if n_src_norm == n_dest_norm {
                return Err(anyhow!("Cannot copy/move source into itself"));
            }

            // For subdirectory check, ensure we check with trailing separator to avoid false prefixes
            let sep = if cfg!(windows) { "\\" } else { "/" };
            let n_src_sep = if n_src_norm.ends_with(sep) {
                n_src_norm.clone()
            } else {
                format!("{n_src_norm}{sep}")
            };

            if n_dest_norm.starts_with(&n_src_sep) {
                return Err(anyhow!("Cannot copy/move into subdirectory of itself"));
            }

            if let Some(file_name) = src_abs.file_name() {
                let effective_dest = dest_abs.join(file_name);
                if let Ok(eff_dest_abs) = effective_dest.canonicalize() {
                    let s_eff = eff_dest_abs.to_string_lossy();
                    #[cfg(windows)]
                    let n_eff = {
                        let s = s_eff.to_lowercase().replace('/', "\\");
                        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
                    };
                    #[cfg(not(windows))]
                    let n_eff = s_eff.to_string();

                    if n_eff == n_src_norm {
                        return Err(anyhow!("Source and destination are the same"));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Compute the target path for a source file/directory
fn compute_target_path(
    dest_path: &std::path::Path,
    dest_str: &str,
    file_name: &std::ffi::OsStr,
    treat_as_dir: bool,
    is_dir_now: bool,
    dest_is_local: bool,
) -> PathBuf {
    if treat_as_dir || is_dir_now {
        if dest_is_local {
            dest_path.join(file_name)
        } else {
            // For remote, use string join to avoid Windows PathBuf join issues
            let base = dest_str.trim_end_matches('/');
            let fname = file_name.to_string_lossy();
            PathBuf::from(format!("{base}/{fname}"))
        }
    } else {
        dest_path.to_path_buf()
    }
}

/// Ensure destination directory exists
async fn ensure_dest_directory(
    dest_fs: &dyn FileSystemProvider,
    dest_path: &std::path::Path,
    treat_as_dir: bool,
    tx: &crate::tasks::EventBus,
    id: usize,
) -> bool {
    if treat_as_dir {
        if !dest_fs.exists(dest_path).await
            && let Err(e) = dest_fs.create_dir_all(dest_path).await
        {
            tx.send(crate::tasks::UiEvent::Task(
                crate::tasks::TaskEvent::UpdateStatus {
                    task_id: id,
                    status: crate::tasks::TaskStatus::Failed(format!(
                        "Failed to create destination directory: {e}",
                    )),
                },
            ));
            return false;
        }
    } else if let Some(parent) = dest_path.parent()
        && !dest_fs.exists(parent).await
    {
        let _ = dest_fs.create_dir_all(parent).await;
    }
    true
}

/// Try rsync for directory transfer
/// Returns: Some(true) if rsync succeeded, Some(false) if skipped, None if should fall through to `recursive_op`
async fn try_rsync_directory(
    transfer: &crate::fs::ops::Transfer<'_>,
    src: &std::path::Path,
    target: &std::path::Path,
    decision_state: &mut crate::fs::ops::DecisionState,
) -> Option<bool> {
    // Check for conflicts before rsync transfer
    if transfer.dest_fs.exists(target).await {
        match transfer
            .request_decision(
                crate::fs::ops::DecisionKind::Conflict,
                decision_state,
                target,
                None,
            )
            .await
        {
            crate::fs::ops::Decision::Overwrite | crate::fs::ops::Decision::OverwriteAll => {}
            crate::fs::ops::Decision::Skip
            | crate::fs::ops::Decision::SkipAll
            | crate::fs::ops::Decision::Retry => {
                // Skipped - update progress and return
                let p = transfer
                    .processed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                transfer.tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateProgress {
                        task_id: transfer.id,
                        processed: p,
                        total: transfer.total,
                    },
                ));
                return Some(false);
            }
            crate::fs::ops::Decision::Cancel => return None,
        }
    }

    // Proceed with rsync. The pre-operation walk was skipped for rsync-eligible
    // transfers, so `total_bytes` is 0 and rsync reports its own byte totals.
    let progress = transfer.progress_ctx();

    if crate::fs::rsync::rsync_transfer(transfer.src_fs, transfer.dest_fs, src, target, &progress)
        .await
        .is_ok()
    {
        // Rsync succeeded - count directory as 1 item for progress
        let p = transfer
            .processed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        transfer.tx.send(crate::tasks::UiEvent::Task(
            crate::tasks::TaskEvent::UpdateProgress {
                task_id: transfer.id,
                processed: p,
                total: transfer.total,
            },
        ));
        return Some(true);
    }
    // Rsync failed - fall through to recursive_op
    None
}

pub async fn handle_copy_move_event(
    code: KeyCode,
    modifiers: Modifiers,
    app: &mut AppState,
) -> bool {
    match code {
        KeyCode::Escape => {
            app.popups.reset_popup(crate::app::PopupKind::CopyMove);
        }
        KeyCode::Enter => {
            let dest_input = app.popups.copy_move.destination_input.text().to_string();
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
            let dest_provider = app.inactive_tab().provider.clone();
            let dest_is_local = dest_provider.is_local();
            let resolved = if is_absolute_dest(&dest_path, dest_is_local) {
                dest_path.clone()
            } else {
                join_dest(&app.active_tab().current_dir, &dest_path, dest_is_local)
            };
            let dest_abs = if dest_is_local {
                if let Ok(p) = resolved.canonicalize() {
                    crate::fs::utils::strip_extended_prefix(p)
                } else {
                    resolved
                }
            } else {
                // For remote, paths are absolute Unix paths
                resolved
            };
            let src_provider = app.active_tab().provider.clone();
            if let Err(err) = validate_copy_move(
                &app.popups.copy_move.source_paths,
                &src_provider,
                &dest_abs,
                &dest_provider,
            )
            .await
            {
                app.popups.copy_move.error = Some(err.to_string());
                return false;
            }

            app.popups
                .copy_move
                .destination_input
                .set_text(dest_abs.to_string_lossy().to_string());

            spawn_copy_move_task(
                app,
                app.active_tab().provider.clone(),
                app.inactive_tab().provider.clone(),
                app.popups.copy_move.source_paths.clone(),
                app.popups.copy_move.destination_input.text().to_string(),
                app.popups.copy_move.action,
            );
            app.popups.reset_popup(crate::app::PopupKind::CopyMove);
        }
        _ => {
            let _ = app
                .popups
                .copy_move
                .destination_input
                .handle_key(code, modifiers, false);
            // Clear error on any input
            if app.popups.copy_move.error.is_some() {
                app.popups.copy_move.error = None;
            }
        }
    }
    false
}

/// The task-level state for one spawned copy/move: the shared [`Transfer`] plus
/// how a source name maps onto the requested destination.
///
/// [`Transfer`]: crate::fs::ops::Transfer
struct CopyMoveTask<'a> {
    transfer: crate::fs::ops::Transfer<'a>,
    dest_path: &'a PathBuf,
    dest_str: &'a str,
    treat_as_dir: bool,
    dest_is_dir: bool,
    use_rsync: bool,
}

async fn process_single_path(
    src: &PathBuf,
    ctx: &CopyMoveTask<'_>,
    decision_state: &mut crate::fs::ops::DecisionState,
) -> Result<(), String> {
    let Some(file_name) = src.file_name() else {
        return Ok(());
    };

    // Compute target path
    let target = compute_target_path(
        ctx.dest_path,
        ctx.dest_str,
        file_name,
        ctx.treat_as_dir,
        ctx.dest_is_dir,
        ctx.transfer.dest_fs.is_local(),
    );

    // Try rsync for directories when applicable
    let src_is_dir = ctx.transfer.src_fs.is_dir(src).await;
    if ctx.use_rsync && src_is_dir {
        let rsync_res = try_rsync_directory(&ctx.transfer, src, &target, decision_state).await;

        match rsync_res {
            Some(true | false) => return Ok(()), // Succeeded or Skipped
            None => {
                // Either cancelled OR failed (and should fall back)
                if ctx
                    .transfer
                    .cancel
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    return Ok(());
                }
                // Fall through to recursive_op
            }
        }
    }

    // Recursive copy/move (also handles per-file rsync as fallback)
    let ops_ctx = crate::fs::ops::RecursiveOpContext {
        transfer: ctx.transfer,
        src,
        dest: &target,
    };
    crate::fs::ops::recursive_op(ops_ctx, decision_state)
        .await
        .map_err(|e| e.to_string())
}

/// Precompute the task totals and the per-path subtree counts in a single
/// walk of the source (the map lets the rename-move optimization credit a
/// whole renamed subtree without a second walk). Skipped for rsync-eligible
/// transfers: rsync provides its own byte-level progress and walking remote
/// trees is slow, so only the top-level item count is used.
async fn compute_task_totals(
    src_provider: &Arc<dyn FileSystemProvider>,
    paths: &[PathBuf],
    use_rsync: bool,
) -> (usize, u64, crate::fs::ops::SubtreeCounts) {
    if use_rsync {
        (paths.len(), 0, crate::fs::ops::SubtreeCounts::new())
    } else {
        let subtree_counts = crate::fs::ops::count_subtrees(src_provider.as_ref(), paths).await;
        let (count, bytes) = paths.iter().fold((0usize, 0u64), |(c, b), p| {
            let (ic, ib) = subtree_counts.get(p).copied().unwrap_or((0, 0));
            (c + ic, b + ib)
        });
        (count, bytes, subtree_counts)
    }
}

pub fn spawn_copy_move_task(
    app: &mut AppState,
    src_provider: Arc<dyn FileSystemProvider>,
    dest_provider: Arc<dyn FileSystemProvider>,
    paths: Vec<PathBuf>,
    dest_str: String,
    action: CopyMoveAction,
) {
    let task_name = match action {
        CopyMoveAction::Copy => format!("Copying {} items", paths.len()),
        CopyMoveAction::Move => format!("Moving {} items", paths.len()),
    };

    // Deselect files in active panel
    {
        let entries = &mut app.active_tab_mut().entries;
        for entry in entries.iter_mut() {
            if entry.selected {
                entry.selected = false;
            }
        }
    }

    // Create channel for decisions
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(1);

    let id = app
        .tasks
        .task_manager
        .spawn_task(&task_name, move |cancel, tx, id| async move {
            let dest_path = std::path::PathBuf::from(&dest_str);

            // Check if rsync can be used for this transfer
            let use_rsync = crate::fs::rsync::should_use_rsync(
                src_provider.as_ref(),
                dest_provider.as_ref(),
                action,
            );

            // Precompute totals and per-path subtree counts in a single walk.
            let (total_items, total_bytes, subtree_counts) =
                compute_task_totals(&src_provider, &paths, use_rsync).await;
            let processed_items = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let processed_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

            // State for "Apply to all" decisions
            // We use a struct to hold this state across recursions
            let mut decision_state = crate::fs::ops::DecisionState {
                overwrite_all: false,
                skip_all: false,

                last_update: std::time::Instant::now(),
            };

            // We need `decision_rx` to be mutual, so we wrap it
            let decision_rx = std::sync::Arc::new(tokio::sync::Mutex::new(decision_rx));

            // Ensure dest dir exists if multiple items or if treated as dir
            let dest_is_dir = dest_provider.is_dir(&dest_path).await;
            let dest_ends_with_slash = dest_str.ends_with(std::path::MAIN_SEPARATOR);
            let treat_as_dir = paths.len() > 1 || dest_is_dir || dest_ends_with_slash;

            if !ensure_dest_directory(dest_provider.as_ref(), &dest_path, treat_as_dir, &tx, id)
                .await
            {
                return;
            }

            let mut failures = Vec::new();

            // One transfer owns the fs pair, the task-wide counters and the
            // decision channel; every path of this task shares it.
            let transfer = crate::fs::ops::Transfer {
                src_fs: src_provider.as_ref(),
                dest_fs: dest_provider.as_ref(),
                action,
                cancel: &cancel,
                tx: &tx,
                id,
                total: total_items,
                total_bytes,
                processed: &processed_items,
                processed_bytes: &processed_bytes,
                decision_rx: &decision_rx,
                subtree_counts: &subtree_counts,
            };

            let ctx = CopyMoveTask {
                transfer,
                dest_path: &dest_path,
                dest_str: &dest_str,
                treat_as_dir,
                dest_is_dir,
                use_rsync,
            };

            for src in &paths {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = process_single_path(src, &ctx, &mut decision_state).await {
                    failures.push(e);
                }
            }

            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateStatus {
                        task_id: id,
                        status: crate::tasks::TaskStatus::Cancelled,
                    },
                ));
            } else if failures.is_empty() {
                tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateStatus {
                        task_id: id,
                        status: crate::tasks::TaskStatus::Completed,
                    },
                ));
            } else {
                // ... error handling
                let msg = format!("Failed with {} errors", failures.len());
                tx.send(crate::tasks::UiEvent::Task(
                    crate::tasks::TaskEvent::UpdateStatus {
                        task_id: id,
                        status: crate::tasks::TaskStatus::Failed(msg),
                    },
                ));
            }
        });

    // Store decision tx
    app.tasks
        .task_manager
        .task_decision_txs
        .insert(id, decision_tx);
}

#[cfg(test)]
mod tests;
