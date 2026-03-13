//! Copy/move popup handler and spawning logic

use crate::app::AppState;
use crate::clipboard::FileClipboardData;
use crate::fs::fs_provider::FileSystemProvider;
use crate::fs::traits::FileSystem;
use crate::state::CopyMoveAction;
use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyModifiers};
use std::path::PathBuf;
use std::sync::Arc;

fn get_paths_to_act_on(app: &AppState) -> Vec<PathBuf> {
    let tab = app.active_tab();
    let current_dir = &tab.current_dir;
    let is_local = tab.provider.is_local();

    let selected_entries = tab.get_selected_entries();
    let entries = if selected_entries.is_empty() {
        if let Some(entry) = tab.current_entry() {
            if entry.name == ".." {
                vec![]
            } else {
                vec![entry]
            }
        } else {
            vec![]
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
    init_copy_move(app, crate::app::CopyMoveAction::Copy);
}

pub fn handle_init_move(app: &mut AppState) {
    init_copy_move(app, crate::app::CopyMoveAction::Move);
}

pub fn init_copy_move(app: &mut AppState, action: crate::app::CopyMoveAction) {
    let paths = get_paths_to_act_on(app);

    if paths.is_empty() {
        return;
    }

    // Get inactive panel path
    let inactive_tab = app.inactive_tab();
    let dest = inactive_tab.current_dir.to_string_lossy().to_string();

    app.popups.copy_move.source_paths = paths;
    app.popups.copy_move.action = action;
    app.popups.copy_move.destination_input = dest;
    app.popups.copy_move.cursor_position = app.popups.copy_move.destination_input.chars().count();
    app.popups.copy_move.input_selected = false;
    app.popups.copy_move.is_visible = true;
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
    app.active_tab_mut().clipboard_msg = Some((
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

    let _ = app.clipboard.set(data);
}

pub fn handle_paste(app: &mut AppState) {
    if let Ok(Some(data)) = app.clipboard.get() {
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
        ) {
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
            let _ = app.clipboard.clear();
        }
    }
}

/// Validates that source paths are not being copied/moved into themselves or subdirectories of themselves.
/// Returns `Err(error_message)` if validation fails, Ok(()) otherwise.
fn validate_copy_move(
    src_paths: &[PathBuf],
    src_provider: &Arc<dyn FileSystemProvider>,
    dest_path: &std::path::Path,
    dest_provider: &Arc<dyn FileSystemProvider>,
) -> Result<()> {
    if src_provider.context_key() != dest_provider.context_key() {
        return Ok(());
    }

    let dest_abs = if let Ok(p) = dest_provider.canonicalize(dest_path) {
        p
    } else if dest_path.is_absolute() {
        dest_path.to_path_buf()
    } else {
        // Fallback to raw path if we can't do better
        dest_path.to_path_buf()
    };

    for src in src_paths {
        if let Ok(src_abs) = src_provider.canonicalize(src) {
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
async fn ensure_dest_directory<F: crate::fs::traits::FileSystem>(
    dest_fs: &F,
    dest_path: &std::path::Path,
    treat_as_dir: bool,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    id: usize,
) -> bool {
    if treat_as_dir {
        if !dest_fs.try_exists(dest_path).await.unwrap_or(false)
            && let Err(e) = dest_fs.create_dir_all(dest_path).await
        {
            let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                id,
                crate::tasks::TaskStatus::Failed(format!(
                    "Failed to create destination directory: {e}",
                )),
            ));
            return false;
        }
    } else if let Some(parent) = dest_path.parent()
        && !dest_fs.try_exists(parent).await.unwrap_or(false)
    {
        let _ = dest_fs.create_dir_all(parent).await;
    }
    true
}

/// Handle conflict resolution for rsync directory transfers
/// Returns: Some(true) to proceed, Some(false) to skip, None to cancel
async fn handle_rsync_conflict<F: crate::fs::traits::FileSystem>(
    dest_fs: &F,
    target: &std::path::Path,
    decision_state: &mut crate::fs::ops::DecisionState,
    decision_rx: &std::sync::Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
    >,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    id: usize,
) -> Option<bool> {
    let target_exists = dest_fs.try_exists(target).await.unwrap_or(false);
    if !target_exists {
        return Some(true);
    }

    // Check decision state for overwrite/skip all
    if decision_state.skip_all {
        return Some(false);
    }
    if decision_state.overwrite_all {
        return Some(true);
    }

    // Ask user about conflict
    let _ = tx.send(crate::tasks::TaskEvent::Conflict(
        id,
        target.to_path_buf(),
        crate::tasks::ConflictType::FileExists,
    ));

    // Wait for decision
    let decision = decision_rx.lock().await.recv().await;
    match decision {
        Some(crate::tasks::TaskDecision::Overwrite) => Some(true),
        Some(crate::tasks::TaskDecision::OverwriteAll) => {
            decision_state.overwrite_all = true;
            Some(true)
        }
        Some(crate::tasks::TaskDecision::SkipAll) => {
            decision_state.skip_all = true;
            Some(false)
        }
        Some(crate::tasks::TaskDecision::Cancel) => {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                id,
                crate::tasks::TaskStatus::Cancelled,
            ));
            None
        }
        _ => Some(false),
    }
}

/// Try rsync for directory transfer
/// Returns: Some(true) if rsync succeeded, Some(false) if skipped, None if should fall through to `recursive_op`
async fn try_rsync_directory<F: crate::fs::traits::FileSystem>(
    ctx: &RsyncContext<'_, F>,
    decision_state: &mut crate::fs::ops::DecisionState,
) -> Option<bool> {
    // Check for conflicts before rsync transfer
    let should_proceed = handle_rsync_conflict(
        ctx.fs.dest_fs,
        ctx.fs.target,
        decision_state,
        ctx.task.decision_rx,
        ctx.task.tx,
        ctx.task.cancel,
        ctx.task.id,
    )
    .await;

    match should_proceed {
        None => None, // Cancelled
        Some(false) => {
            // Skipped - update progress and return
            let p = ctx
                .task
                .processed_items
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            let _ = ctx.task.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                ctx.task.id,
                p,
                ctx.task.total_items,
            ));
            Some(false)
        }
        Some(true) => {
            // Proceed with rsync
            let progress = crate::fs::traits::TaskProgressContext {
                id: ctx.task.id,
                tx: ctx.task.tx.clone(),
                cancel: ctx.task.cancel.clone(),
                processed_bytes: ctx.task.processed_bytes.clone(),
                processed_items: ctx.task.processed_items.clone(),
            };

            if crate::fs::fs_rsync::rsync_transfer(
                ctx.fs.src_fs,
                ctx.fs.dest_fs,
                ctx.fs.src,
                ctx.fs.target,
                &progress,
            )
            .await
            .is_ok()
            {
                // Rsync succeeded - count directory as 1 item for progress
                let p = ctx
                    .task
                    .processed_items
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                let _ = ctx.task.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                    ctx.task.id,
                    p,
                    ctx.task.total_items,
                ));
                return Some(true);
            }
            // Rsync failed - fall through to recursive_op
            None
        }
    }
}

pub fn handle_copy_move_event(code: KeyCode, modifiers: KeyModifiers, app: &mut AppState) -> bool {
    match code {
        KeyCode::Esc => {
            app.popups.copy_move.reset();
        }
        KeyCode::Enter => {
            let dest_input = app.popups.copy_move.destination_input.clone();
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
            let dest_abs = if dest_provider.is_local() {
                if let Ok(p) = dest_path.canonicalize() {
                    p
                } else if dest_path.is_absolute() {
                    dest_path.clone()
                } else {
                    app.active_tab().current_dir.join(&dest_path)
                }
            } else {
                // For remote, treat as absolute Unix path
                dest_path.clone()
            };
            let src_provider = app.active_tab().provider.clone();
            if let Err(err) = validate_copy_move(
                &app.popups.copy_move.source_paths,
                &src_provider,
                &dest_abs,
                &dest_provider,
            ) {
                app.popups.copy_move.error = Some(err.to_string());
                return false;
            }

            app.popups.copy_move.destination_input = dest_abs.to_string_lossy().to_string();
            app.popups.copy_move.cursor_position =
                app.popups.copy_move.destination_input.chars().count();

            spawn_copy_move_task(
                app,
                app.active_tab().provider.clone(),
                app.inactive_tab().provider.clone(),
                app.popups.copy_move.source_paths.clone(),
                app.popups.copy_move.destination_input.clone(),
                app.popups.copy_move.action,
            );
            app.popups.copy_move.reset();
        }
        _ => {
            crate::handlers::input_utils::handle_text_input(
                code,
                modifiers,
                &mut app.popups.copy_move.destination_input,
                &mut app.popups.copy_move.cursor_position,
                false,
            );
            // Clear error on any input
            if app.popups.copy_move.error.is_some() {
                app.popups.copy_move.error = None;
            }
        }
    }
    false
}

struct ProcessPathContext<'a> {
    src_fs: &'a crate::fs::provider::ProviderFileSystem,
    dest_fs: &'a crate::fs::provider::ProviderFileSystem,
    dest_path: &'a std::path::PathBuf,
    dest_str: &'a str,
    treat_as_dir: bool,
    dest_is_dir: bool,
    use_rsync: bool,
    action: crate::state::CopyMoveAction,
    id: usize,
    total_items: usize,
    total_bytes: u64,
    tx: &'a tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    cancel: &'a Arc<std::sync::atomic::AtomicBool>,
    decision_rx:
        &'a Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>>,
    processed_items: &'a Arc<std::sync::atomic::AtomicUsize>,
    processed_bytes: &'a Arc<std::sync::atomic::AtomicU64>,
}

struct RsyncFsContext<'a, F: crate::fs::traits::FileSystem> {
    src_fs: &'a F,
    dest_fs: &'a F,
    src: &'a std::path::Path,
    target: &'a std::path::Path,
}

struct RsyncTaskContext<'a> {
    decision_rx: &'a std::sync::Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
    >,
    tx: &'a tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    cancel: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    processed_items: &'a std::sync::Arc<std::sync::atomic::AtomicUsize>,
    processed_bytes: &'a std::sync::Arc<std::sync::atomic::AtomicU64>,
    id: usize,
    total_items: usize,
}

impl<'a, F: crate::fs::traits::FileSystem> RsyncFsContext<'a, F> {
    pub fn new(
        src_fs: &'a F,
        dest_fs: &'a F,
        src: &'a std::path::Path,
        target: &'a std::path::Path,
    ) -> Self {
        Self {
            src_fs,
            dest_fs,
            src,
            target,
        }
    }
}

impl<'a> RsyncTaskContext<'a> {
    pub fn new(
        decision_rx: &'a std::sync::Arc<
            tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
        >,
        tx: &'a tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
        cancel: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
        processed_items: &'a std::sync::Arc<std::sync::atomic::AtomicUsize>,
        processed_bytes: &'a std::sync::Arc<std::sync::atomic::AtomicU64>,
        id: usize,
        total_items: usize,
    ) -> Self {
        Self {
            decision_rx,
            tx,
            cancel,
            processed_items,
            processed_bytes,
            id,
            total_items,
        }
    }
}

struct RsyncContext<'a, F: crate::fs::traits::FileSystem> {
    fs: RsyncFsContext<'a, F>,
    task: RsyncTaskContext<'a>,
}

async fn process_single_path(
    src: &PathBuf,
    ctx: &ProcessPathContext<'_>,
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
        ctx.dest_fs.0.is_local(),
    );

    // Try rsync for directories when applicable
    let src_is_dir = ctx.src_fs.is_dir(src).await.unwrap_or(false);
    if ctx.use_rsync && src_is_dir {
        let fs_ctx = RsyncFsContext::new(ctx.src_fs, ctx.dest_fs, src, &target);
        let task_ctx = RsyncTaskContext::new(
            ctx.decision_rx,
            ctx.tx,
            ctx.cancel,
            ctx.processed_items,
            ctx.processed_bytes,
            ctx.id,
            ctx.total_items,
        );
        let rsync_ctx = RsyncContext {
            fs: fs_ctx,
            task: task_ctx,
        };
        let rsync_res = try_rsync_directory(&rsync_ctx, decision_state).await;

        match rsync_res {
            Some(true | false) => return Ok(()), // Succeeded or Skipped
            None => {
                // Either cancelled OR failed (and should fall back)
                if ctx.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Ok(());
                }
                // Fall through to recursive_op
            }
        }
    }

    // Recursive copy/move (also handles per-file rsync as fallback)
    let ops_ctx = crate::fs::ops::RecursiveOpContext {
        src_fs: ctx.src_fs,
        dest_fs: ctx.dest_fs,
        src,
        dest: &target,
        action: ctx.action,
        cancel: ctx.cancel,
        tx: ctx.tx,
        id: ctx.id,
        total: ctx.total_items,
        total_bytes: ctx.total_bytes,
        processed: ctx.processed_items,
        processed_bytes: ctx.processed_bytes,
        decision_rx: ctx.decision_rx,
    };
    crate::fs::ops::recursive_op(ops_ctx, decision_state)
        .await
        .map_err(|e| e.to_string())
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
        .task_manager
        .spawn_task(&task_name, move |cancel, tx, id| async move {
            let src_fs = crate::fs::provider::ProviderFileSystem(src_provider);
            let dest_fs = crate::fs::provider::ProviderFileSystem(dest_provider);
            let dest_path = std::path::PathBuf::from(&dest_str);

            // Check if rsync can be used for this transfer
            let use_rsync = crate::fs::fs_rsync::should_use_rsync(&src_fs, &dest_fs, action);

            // Pre-calculation of total items using the source filesystem
            // Skip this for rsync-eligible transfers to avoid slow remote directory traversal
            // (rsync provides its own byte-level progress)
            let (total_items, total_bytes) = if use_rsync {
                // For rsync, just count top-level items - rsync handles progress internally
                (paths.len(), 0)
            } else {
                crate::fs::ops::count_items_and_size(&src_fs, &paths).await
            };
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
            let dest_is_dir = dest_fs.is_dir(&dest_path).await.unwrap_or(false);
            let dest_ends_with_slash = dest_str.ends_with(std::path::MAIN_SEPARATOR);
            let treat_as_dir = paths.len() > 1 || dest_is_dir || dest_ends_with_slash;

            if !ensure_dest_directory(&dest_fs, &dest_path, treat_as_dir, &tx, id).await {
                return;
            }

            let mut failures = Vec::new();

            let ctx = ProcessPathContext {
                src_fs: &src_fs,
                dest_fs: &dest_fs,
                dest_path: &dest_path,
                dest_str: &dest_str,
                treat_as_dir,
                dest_is_dir,
                use_rsync,
                action,
                id,
                total_items,
                total_bytes,
                tx: &tx,
                cancel: &cancel,
                decision_rx: &decision_rx,
                processed_items: &processed_items,
                processed_bytes: &processed_bytes,
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
                let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                    id,
                    crate::tasks::TaskStatus::Cancelled,
                ));
            } else if failures.is_empty() {
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
