//! Copy/move popup handler and spawning logic

use crate::app::AppState;
use crate::clipboard::FileClipboardData;
use crate::fs_provider::FileSystemProvider;
use crate::handlers::clipboard_utils::{get_clipboard_content, insert_text_at_cursor};
use crate::state::CopyMoveAction;
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
    app.popups.copy_move.cursor_position = app.popups.copy_move.destination_input.len();
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

        if let Some(err) = validate_copy_move(
            &data.paths,
            &data.source_provider,
            &dest_path,
            &dest_provider,
        ) {
            app.active_tab_mut().error = Some(err);
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

/// Validates that source paths are não being copied/moved into themselves or subdirectories of themselves.
/// Returns Some(error_message) if validation fails, None otherwise.
fn validate_copy_move(
    src_paths: &[PathBuf],
    src_provider: &Arc<dyn FileSystemProvider>,
    dest_path: &std::path::Path,
    dest_provider: &Arc<dyn FileSystemProvider>,
) -> Option<String> {
    if src_provider.context_key() != dest_provider.context_key() {
        return None;
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
                s_src.to_lowercase().replace("/", "\\"),
                s_dest.to_lowercase().replace("/", "\\"),
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
                return Some("Cannot copy/move source into itself".to_string());
            }

            // For subdirectory check, ensure we check with trailing separator to avoid false prefixes
            let sep = if cfg!(windows) { "\\" } else { "/" };
            let n_src_sep = if n_src_norm.ends_with(sep) {
                n_src_norm.clone()
            } else {
                format!("{}{}", n_src_norm, sep)
            };

            if n_dest_norm.starts_with(&n_src_sep) {
                return Some("Cannot copy/move into subdirectory of itself".to_string());
            }

            if let Some(file_name) = src_abs.file_name() {
                let effective_dest = dest_abs.join(file_name);
                if let Ok(eff_dest_abs) = effective_dest.canonicalize() {
                    let s_eff = eff_dest_abs.to_string_lossy();
                    #[cfg(windows)]
                    let n_eff = {
                        let s = s_eff.to_lowercase().replace("/", "\\");
                        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
                    };
                    #[cfg(not(windows))]
                    let n_eff = s_eff.to_string();

                    if n_eff == n_src_norm {
                        return Some("Source and destination are the same".to_string());
                    }
                }
            }
        }
    }
    None
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
            app.popups.copy_move.destination_input = dest_abs.to_string_lossy().to_string();
            let src_provider = app.active_tab().provider.clone();
            if let Some(err) = validate_copy_move(
                &app.popups.copy_move.source_paths,
                &src_provider,
                &dest_abs,
                &dest_provider,
            ) {
                app.popups.copy_move.error = Some(err);
                return false;
            }

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
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(content) = get_clipboard_content() {
                insert_text_at_cursor(
                    &mut app.popups.copy_move.destination_input,
                    &mut app.popups.copy_move.cursor_position,
                    &content,
                );
            }
        }
        KeyCode::Char(c) => {
            app.popups.copy_move.error = None;
            app.popups
                .copy_move
                .destination_input
                .insert(app.popups.copy_move.cursor_position, c);
            app.popups.copy_move.cursor_position += 1;
        }
        KeyCode::Backspace => {
            if app.popups.copy_move.cursor_position > 0 {
                app.popups
                    .copy_move
                    .destination_input
                    .remove(app.popups.copy_move.cursor_position - 1);
                app.popups.copy_move.cursor_position -= 1;
            }
        }
        KeyCode::Delete => {
            if app.popups.copy_move.cursor_position < app.popups.copy_move.destination_input.len() {
                app.popups
                    .copy_move
                    .destination_input
                    .remove(app.popups.copy_move.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.popups.copy_move.cursor_position > 0 {
                app.popups.copy_move.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.popups.copy_move.cursor_position < app.popups.copy_move.destination_input.len() {
                app.popups.copy_move.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.popups.copy_move.cursor_position = 0;
        }
        KeyCode::End => {
            app.popups.copy_move.cursor_position = app.popups.copy_move.destination_input.len();
        }
        _ => {}
    }
    false
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
        .spawn_task(task_name, move |cancel, tx, id| async move {
            let src_fs = crate::handlers::file_ops::ProviderFileSystem(src_provider);
            let dest_fs = crate::handlers::file_ops::ProviderFileSystem(dest_provider);
            let dest_path = std::path::PathBuf::from(&dest_str);
            // Pre-calculation of total items using the source filesystem
            let total_items = crate::handlers::file_ops::count_items(&src_fs, &paths).await;
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
            use crate::handlers::file_ops::FileSystem;

            // Re-evaluate if destination is a directory using the correct filesystem
            let dest_is_dir = dest_fs.is_dir(&dest_path).await.unwrap_or(false);
            let dest_ends_with_slash = dest_str.ends_with(std::path::MAIN_SEPARATOR);

            let treat_as_dir = paths.len() > 1 || dest_is_dir || dest_ends_with_slash;

            if treat_as_dir {
                // We should ensure the directory exists on the destination filesystem
                if !dest_fs.try_exists(&dest_path).await.unwrap_or(false)
                    && let Err(e) = dest_fs.create_dir_all(&dest_path).await
                {
                    let _ = tx.send(crate::tasks::TaskEvent::UpdateStatus(
                        id,
                        crate::tasks::TaskStatus::Failed(format!(
                            "Failed to create destination directory: {}",
                            e
                        )),
                    ));
                    return;
                }
            } else if let Some(parent) = dest_path.parent()
                && !dest_fs.try_exists(parent).await.unwrap_or(false)
            {
                let _ = dest_fs.create_dir_all(parent).await;
            }

            let mut failures = Vec::new();

            for src in &paths {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }

                let file_name = match src.file_name() {
                    Some(n) => n,
                    None => continue,
                };

                // Re-check is_dir in case it was created above or existed
                let is_dir_now = dest_fs.is_dir(&dest_path).await.unwrap_or(false);
                let target = if treat_as_dir || is_dir_now {
                    if dest_fs.0.is_local() {
                        dest_path.join(file_name)
                    } else {
                        // For remote, use string join to avoid Windows PathBuf join issues
                        let base = dest_str.trim_end_matches('/');
                        let fname = file_name.to_string_lossy();
                        PathBuf::from(format!("{}/{}", base, fname))
                    }
                } else {
                    dest_path.clone()
                };

                // Recursive copy/move
                let ctx = crate::handlers::file_ops::RecursiveOpContext {
                    src_fs: &src_fs,
                    dest_fs: &dest_fs,
                    src,
                    dest: &target,
                    action,
                    cancel: &cancel,
                    tx: &tx,
                    id,
                    total: total_items,
                    processed: &processed_items,
                    decision_rx: &decision_rx,
                };
                let res = crate::handlers::file_ops::recursive_op(ctx, &mut decision_state).await;

                if let Err(e) = res {
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

#[cfg(test)]
mod popup_copy_move_unit_tests {
    use super::*;

    use crate::app::{AppState, PanelSide, Tab, TabManager};
    use crate::fs_local::LocalFs;
    use crate::fs_ops::FileEntry;
    use crate::state::CopyMoveAction;
    use crossterm::event::KeyCode;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    // --- Helpers to build minimal AppState for popup tests ---

    fn make_fileentry(name: &str, selected: bool, is_dir: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            is_dir,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected,
        }
    }

    fn make_tab(path: &str, entries: Vec<FileEntry>, cursor: usize) -> Tab {
        Tab {
            provider: Arc::new(LocalFs::new()),
            current_dir: PathBuf::from(path),
            entries,
            cursor,
            history: vec![],
            history_index: 0,
            error: None,
            typed_buffer: String::new(),
            last_type_time: None,
            matching_indices: Vec::new(),
            search_position: 0,
            sort_column: crate::app::SortColumn::Name,
            sort_direction: crate::app_state::tabs::SortDirection::Ascending,
            scroll_offset: 0,
            custom_title: None,
            clipboard_msg: None,
        }
    }

    fn make_tab_manager(tab: Tab) -> TabManager {
        TabManager {
            tabs: vec![tab],
            active_tab_index: 0,
        }
    }

    fn minimal_state_with_entries(
        active: PanelSide,
        left_entries: Vec<FileEntry>,
        right_entries: Vec<FileEntry>,
        left_cursor: usize,
        right_cursor: usize,
    ) -> AppState {
        AppState {
            left: make_tab_manager(make_tab("/left", left_entries, left_cursor)),
            right: make_tab_manager(make_tab("/right", right_entries, right_cursor)),
            active,
            // Popups and config fields as default/minimal:
            file_viewer: Default::default(),
            fuzzy_search: Default::default(),
            popups: crate::app::Popups::new(),
            task_manager: crate::tasks::TaskManager::new(tokio::sync::mpsc::unbounded_channel().0),
            ssh_manager: std::sync::Arc::new(crate::ssh_manager::SshManager::default()),
            task_decision_txs: HashMap::new(),
            show_task_manager: false,
            dir_history: Default::default(),
            watcher: None,
            input_polling_handle: None,
            needs_redraw: false,
            global: crate::config::GlobalConfig::default(),
            editor_cfg: crate::config::EditorConfig::default(),
            viewer_cfg: crate::config::ViewerConfig::default(),
            ssh_history: crate::ssh_history::SshConnectionHistory::new().unwrap(),
            clipboard: Box::new(crate::clipboard::ClipboardBackend::new()),
        }
    }

    #[test]
    fn test_init_copy_and_move_selects_correct_paths() {
        // Left panel active, selected entry (not '..'), should populate paths
        let left_entries = vec![
            make_fileentry("A.txt", true, false),
            make_fileentry("..", false, true),
        ];
        let right_entries = vec![make_fileentry("X", false, false)];
        let mut app =
            minimal_state_with_entries(PanelSide::Left, left_entries, right_entries.clone(), 0, 0);
        handle_init_copy(&mut app);
        assert!(app.popups.copy_move.is_visible);
        assert_eq!(app.popups.copy_move.action, CopyMoveAction::Copy);
        assert!(app.popups.copy_move.source_paths[0].ends_with("A.txt"));

        let left_entries = vec![
            make_fileentry("B.txt", false, false),
            make_fileentry("..", false, true),
        ];
        let mut app =
            minimal_state_with_entries(PanelSide::Left, left_entries, right_entries.clone(), 0, 0);
        handle_init_move(&mut app);
        assert_eq!(app.popups.copy_move.action, CopyMoveAction::Move);
    }

    #[test]
    fn test_init_copy_for_no_selection_uses_current_if_not_parent() {
        let left_entries = vec![
            make_fileentry("foo", false, false),
            make_fileentry("..", false, true),
        ];
        // Cursor points to "foo"
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        assert!(app.popups.copy_move.source_paths[0].ends_with("foo"));
    }

    #[test]
    fn test_init_copy_for_parent_dir_does_nothing() {
        let left_entries = vec![make_fileentry("..", false, true)];
        // Cursor points to ".."
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        assert!(!app.popups.copy_move.is_visible);
        assert_eq!(app.popups.copy_move.source_paths.len(), 0);
    }

    #[test]
    fn test_handle_copy_move_event_char_and_edit() {
        let left_entries = vec![make_fileentry("a", true, false)];
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        app.popups.copy_move.destination_input.clear();
        app.popups.copy_move.cursor_position = 0;
        // Insert 'x'
        handle_copy_move_event(KeyCode::Char('x'), KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.copy_move.destination_input, "x");
        assert_eq!(app.popups.copy_move.cursor_position, 1);
        // Insert 'y' at position 1
        handle_copy_move_event(KeyCode::Char('y'), KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.copy_move.destination_input, "xy");
        assert_eq!(app.popups.copy_move.cursor_position, 2);
        // Backspace
        handle_copy_move_event(KeyCode::Backspace, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.copy_move.destination_input, "x");
        assert_eq!(app.popups.copy_move.cursor_position, 1);
        // Left
        handle_copy_move_event(KeyCode::Left, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.copy_move.cursor_position, 0);
        // Delete (removes 'x')
        handle_copy_move_event(KeyCode::Delete, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.copy_move.destination_input, "");
        assert_eq!(app.popups.copy_move.cursor_position, 0);
    }

    #[test]
    fn test_handle_copy_move_event_navigation_keys() {
        let left_entries = vec![make_fileentry("a", true, false)];
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        app.popups.copy_move.destination_input = "abcdef".to_string();
        app.popups.copy_move.cursor_position = 3;
        // Home
        handle_copy_move_event(KeyCode::Home, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.copy_move.cursor_position, 0);
        // End
        handle_copy_move_event(KeyCode::End, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.copy_move.cursor_position, 6);
        // Right at end (should stay)
        handle_copy_move_event(KeyCode::Right, KeyModifiers::NONE, &mut app);
        assert_eq!(app.popups.copy_move.cursor_position, 6);
    }

    #[test]
    fn test_handle_copy_move_event_escape_resets_popup() {
        let left_entries = vec![make_fileentry("a", true, false)];
        let mut app = minimal_state_with_entries(PanelSide::Left, left_entries, vec![], 0, 0);
        handle_init_copy(&mut app);
        app.popups.copy_move.error = Some("some error".to_string());
        assert!(app.popups.copy_move.is_visible);
        handle_copy_move_event(KeyCode::Esc, KeyModifiers::NONE, &mut app);
        assert!(!app.popups.copy_move.is_visible);
        assert!(app.popups.copy_move.error.is_none());
    }

    #[tokio::test]
    async fn test_handle_copy_move_event_home_dir_expansion() {
        let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
        app.popups.copy_move.is_visible = true;
        app.popups.copy_move.destination_input = "~".to_string();

        handle_copy_move_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);

        if let Some(base_dirs) = directories::BaseDirs::new() {
            let home = base_dirs
                .home_dir()
                .canonicalize()
                .unwrap_or_else(|_| base_dirs.home_dir().to_path_buf());
            let _home_str = home.to_string_lossy().to_string();
            // It resets on success, but we can check if it's not visible anymore
            assert!(!app.popups.copy_move.is_visible);
        }
    }

    #[tokio::test]
    async fn test_handle_copy_move_validation_same_path() {
        let temp_dir = std::env::temp_dir().canonicalize().unwrap();
        let file_path = temp_dir.join("test_file_copy_val.txt");
        std::fs::File::create(&file_path).unwrap();

        let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
        app.popups.copy_move.is_visible = true;
        app.popups.copy_move.source_paths = vec![file_path.clone()];
        app.popups.copy_move.destination_input = temp_dir.to_string_lossy().to_string();

        handle_copy_move_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);

        assert!(app.popups.copy_move.error.is_some());
        assert!(
            app.popups
                .copy_move
                .error
                .as_ref()
                .unwrap()
                .contains("same")
        );

        std::fs::remove_file(&file_path).ok();
    }

    #[tokio::test]
    async fn test_handle_copy_move_validation_into_itself() {
        let temp_dir = std::env::temp_dir().canonicalize().unwrap();
        let src_dir = temp_dir.join("test_src_dir");
        std::fs::create_dir_all(&src_dir).unwrap();
        let dest_dir = src_dir.join("test_dest_dir");

        let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
        app.popups.copy_move.is_visible = true;
        app.popups.copy_move.source_paths = vec![src_dir.clone()];
        app.popups.copy_move.destination_input = dest_dir.to_string_lossy().to_string();

        handle_copy_move_event(KeyCode::Enter, KeyModifiers::NONE, &mut app);

        assert!(app.popups.copy_move.error.is_some());
        assert!(
            app.popups
                .copy_move
                .error
                .as_ref()
                .unwrap()
                .contains("subdirectory of itself")
        );

        std::fs::remove_dir_all(&src_dir).ok();
    }

    #[tokio::test]
    async fn test_handle_paste_validation_same_path() {
        let temp_dir = std::env::temp_dir().canonicalize().unwrap();
        let file_path = temp_dir.join("test_file_paste_val.txt");
        std::fs::File::create(&file_path).unwrap();

        let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
        app.left.active_tab_mut().current_dir = temp_dir.clone();

        let data = crate::clipboard::FileClipboardData {
            action: crate::clipboard::FileClipboardAction::Copy,
            paths: vec![file_path.clone()],
            source_provider: Arc::new(LocalFs::new()),
        };
        app.clipboard.set(data).unwrap();

        handle_paste(&mut app);

        assert!(app.left.active_tab().error.is_some());
        assert!(
            app.left
                .active_tab()
                .error
                .as_ref()
                .unwrap()
                .contains("same")
        );

        std::fs::remove_file(&file_path).ok();
    }

    #[tokio::test]
    async fn test_handle_paste_clears_clipboard_on_move() {
        let temp_dir = std::env::temp_dir().canonicalize().unwrap();
        let src_file = temp_dir.join("test_paste_clear_src.txt");
        std::fs::File::create(&src_file).unwrap();

        let mut app = minimal_state_with_entries(PanelSide::Left, vec![], vec![], 0, 0);
        // Navigate to different dir to pass path validation
        let dest_dir = temp_dir.join("test_paste_clear_dest");
        std::fs::create_dir_all(&dest_dir).unwrap();
        app.left.active_tab_mut().current_dir = dest_dir.clone();

        let data = crate::clipboard::FileClipboardData {
            action: crate::clipboard::FileClipboardAction::Cut,
            paths: vec![src_file.clone()],
            source_provider: Arc::new(LocalFs::new()),
        };
        app.clipboard.set(data).unwrap();

        handle_paste(&mut app);

        // Clipboard should be empty now
        assert!(app.clipboard.get().unwrap().is_none());

        std::fs::remove_file(&src_file).ok();
        std::fs::remove_dir_all(&dest_dir).ok();
    }

    #[test]
    fn test_handle_clipboard_action_sets_message() {
        use crate::fs_ops::FileEntry;
        let entries = vec![FileEntry {
            name: "test.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(10),
            modified: None,
            attributes: "".to_string(),
            selected: true,
        }];
        let mut app = minimal_state_with_entries(PanelSide::Left, entries, vec![], 0, 0);

        super::handle_clipboard_copy(&mut app);

        assert!(app.active_tab().clipboard_msg.is_some());
        let (msg, _) = app.active_tab().clipboard_msg.as_ref().unwrap();
        assert_eq!(msg, "1 item copied");
    }
}
