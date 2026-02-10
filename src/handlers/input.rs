use crate::app::{AppState, SortColumn};
use crate::config::KeyboardConfig;
use crate::handlers::{
    editor::handle_edit,
    input_utils::keyevent_to_string,
    navigation::{
        handle_directory_up, handle_down, handle_down_search, handle_end, handle_enter_directory,
        handle_history_next, handle_history_previous, handle_home, handle_open_item,
        handle_page_down, handle_page_up, handle_sort, handle_tab, handle_toggle_selection,
        handle_type_char, handle_up, handle_up_search, reset_search,
    },
    popup_copy_move::{
        handle_clipboard_copy, handle_clipboard_cut, handle_init_copy, handle_init_move,
        handle_paste,
    },
    popup_create::{handle_init_create_directory, handle_init_create_file},
    popup_delete::handle_init_delete,
    popup_rename::handle_init_rename,
    popup_ssh::{handle_reconnect_ssh, handle_ssh_connection_init},
    tabs::{handle_close_tab, handle_new_tab, handle_next_tab, handle_prev_tab},
    terminal::handle_open_terminal,
};
use crossterm::event::{KeyCode, KeyModifiers};

pub async fn handle_main_panel_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    input_tx: tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> bool {
    let shortcut = keyevent_to_string(code, modifiers);

    // Empty Trash
    if let Some(keys) = &keyboard.empty_trash
        && keys.contains(&shortcut)
    {
        app.popups.empty_trash.is_visible = true;
        return false;
    }

    // Clear error on any interaction in the main panel
    {
        app.active_tab_mut().error = None;
    }

    // Tab management shortcuts
    if handle_tab_shortcuts(app, keyboard, &shortcut) {
        return false;
    }

    // Clipboard handlers
    if handle_clipboard_shortcuts(app, code, modifiers) {
        return false;
    }

    // Navigation (history, dir up/enter)
    if handle_navigation_shortcuts(app, keyboard, &shortcut) {
        return false;
    }

    // Edit
    if let Some(keys) = &keyboard.edit_file
        && keys.contains(&shortcut)
    {
        // Await edit action, pass input_tx
        handle_edit(app, input_tx.clone()).await;
        return false;
    }

    // Fuzzy search
    if let Some(keys) = &keyboard.search
        && keys.contains(&shortcut)
    {
        app.fuzzy_search.is_visible = true;
        app.fuzzy_search.reset();
        // Initialize with all directories sorted by score
        let context_key = app.active_tab().provider.context_key();
        let results = app.dir_history.fuzzy_search(&context_key, "");
        app.fuzzy_search.filtered_dirs = results.into_iter().map(|(p, _)| p).collect();
        app.fuzzy_search.selected_index = 0;
        return false;
    }

    // File Operations
    if handle_file_ops_shortcuts(app, keyboard, &shortcut) {
        return false;
    }

    // Task Manager
    if let Some(keys) = &keyboard.tasks
        && keys.contains(&shortcut)
    {
        app.show_task_manager = !app.show_task_manager;
        return false;
    }

    // Help
    if let Some(keys) = &keyboard.help
        && keys.contains(&shortcut)
    {
        app.popups.help.is_visible = true;
        return false;
    }

    // Swap Tabs
    if let Some(keys) = &keyboard.swap_tabs
        && keys.contains(&shortcut)
    {
        match app.can_swap_active_tabs() {
            Ok(_) => app.swap_active_tabs(),
            Err(e) => app.active_tab_mut().error = Some(e.to_string()),
        }
        return false;
    }

    // SSH
    if handle_ssh_shortcuts(app, keyboard, &shortcut) {
        return false;
    }

    // Open Terminal
    if let Some(keys) = &keyboard.open_terminal
        && keys.contains(&shortcut)
    {
        handle_open_terminal(app);
        return false;
    }

    // Drive Selection
    if handle_drive_selection(app, keyboard, &shortcut) {
        return false;
    }

    // Sorting
    if handle_sorting_shortcuts(app, keyboard, &shortcut) {
        return false;
    }

    // Select All
    if let Some(keys) = &keyboard.select_all
        && keys.contains(&shortcut)
    {
        app.active_tab_mut().select_all();
        return false;
    }

    // Fallback to basic type/nav handling
    handle_basic_nav(app, code, modifiers);

    false
}

fn handle_tab_shortcuts(app: &mut AppState, keyboard: &KeyboardConfig, shortcut: &str) -> bool {
    if let Some(keys) = &keyboard.new_tab
        && keys.iter().any(|s| s == shortcut)
    {
        handle_new_tab(app);
        return true;
    }
    if let Some(keys) = &keyboard.tab_next
        && keys.iter().any(|s| s == shortcut)
    {
        handle_next_tab(app);
        return true;
    }
    if let Some(keys) = &keyboard.tab_prev
        && keys.iter().any(|s| s == shortcut)
    {
        handle_prev_tab(app);
        return true;
    }
    if let Some(keys) = &keyboard.tab_close
        && keys.iter().any(|s| s == shortcut)
    {
        handle_close_tab(app);
        return true;
    }
    false
}

fn handle_clipboard_shortcuts(app: &mut AppState, code: KeyCode, modifiers: KeyModifiers) -> bool {
    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        handle_clipboard_copy(app);
        return true;
    }
    if code == KeyCode::Char('x') && modifiers.contains(KeyModifiers::CONTROL) {
        handle_clipboard_cut(app);
        return true;
    }
    if code == KeyCode::Char('v') && modifiers.contains(KeyModifiers::CONTROL) {
        handle_paste(app);
        return true;
    }
    false
}

fn handle_navigation_shortcuts(
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    shortcut: &str,
) -> bool {
    if let Some(keys) = &keyboard.back
        && keys.iter().any(|s| s == shortcut)
    {
        handle_history_previous(app);
        return true;
    }
    if let Some(keys) = &keyboard.forward
        && keys.iter().any(|s| s == shortcut)
    {
        handle_history_next(app);
        return true;
    }
    if let Some(keys) = &keyboard.enter_dir
        && keys.iter().any(|s| s == shortcut)
    {
        handle_enter_directory(app);
        return true;
    }
    if let Some(keys) = &keyboard.up_dir
        && keys.iter().any(|s| s == shortcut)
    {
        handle_directory_up(app);
        return true;
    }
    false
}

fn handle_file_ops_shortcuts(
    app: &mut AppState,
    keyboard: &KeyboardConfig,
    shortcut: &str,
) -> bool {
    if let Some(keys) = &keyboard.rename
        && keys.iter().any(|s| s == shortcut)
    {
        handle_init_rename(app);
        return true;
    }
    if let Some(keys) = &keyboard.new_file
        && keys.iter().any(|s| s == shortcut)
    {
        handle_init_create_file(app);
        return true;
    }
    if let Some(keys) = &keyboard.new_dir
        && keys.iter().any(|s| s == shortcut)
    {
        handle_init_create_directory(app);
        return true;
    }
    if let Some(keys) = &keyboard.delete
        && keys.iter().any(|s| s == shortcut)
    {
        handle_init_delete(app, false);
        return true;
    }
    if let Some(keys) = &keyboard.delete_force
        && keys.iter().any(|s| s == shortcut)
    {
        handle_init_delete(app, true);
        return true;
    }
    if let Some(keys) = &keyboard.copy_to
        && keys.iter().any(|s| s == shortcut)
    {
        handle_init_copy(app);
        return true;
    }
    if let Some(keys) = &keyboard.move_to
        && keys.iter().any(|s| s == shortcut)
    {
        handle_init_move(app);
        return true;
    }
    false
}

fn handle_ssh_shortcuts(app: &mut AppState, keyboard: &KeyboardConfig, shortcut: &str) -> bool {
    if let Some(keys) = &keyboard.open_ssh
        && keys.iter().any(|s| s == shortcut)
    {
        handle_ssh_connection_init(app);
        return true;
    }
    if let Some(keys) = &keyboard.reconnect_ssh
        && keys.iter().any(|s| s == shortcut)
    {
        handle_reconnect_ssh(app);
        return true;
    }
    false
}

fn handle_drive_selection(app: &mut AppState, keyboard: &KeyboardConfig, shortcut: &str) -> bool {
    // Left
    if let Some(keys) = &keyboard.change_drive_left
        && keys.iter().any(|s| s == shortcut)
    {
        let drives = crate::drive_select_ui::get_available_drives();
        if !drives.is_empty() {
            app.popups.drive_select.is_visible = true;
            app.popups.drive_select.drives = drives;
            app.popups.drive_select.side = crate::app::PanelSide::Left;
            app.popups.drive_select.selected_index = 0;
        }
        return true;
    }
    // Right
    if let Some(keys) = &keyboard.change_drive_right
        && keys.iter().any(|s| s == shortcut)
    {
        let drives = crate::drive_select_ui::get_available_drives();
        if !drives.is_empty() {
            app.popups.drive_select.is_visible = true;
            app.popups.drive_select.drives = drives;
            app.popups.drive_select.side = crate::app::PanelSide::Right;
            app.popups.drive_select.selected_index = 0;
        }
        return true;
    }
    false
}

fn handle_sorting_shortcuts(app: &mut AppState, keyboard: &KeyboardConfig, shortcut: &str) -> bool {
    if let Some(keys) = &keyboard.sort_name
        && keys.iter().any(|s| s == shortcut)
    {
        handle_sort(app, SortColumn::Name);
        return true;
    }
    if let Some(keys) = &keyboard.sort_ext
        && keys.iter().any(|s| s == shortcut)
    {
        handle_sort(app, SortColumn::Extension);
        return true;
    }
    if let Some(keys) = &keyboard.sort_date
        && keys.iter().any(|s| s == shortcut)
    {
        handle_sort(app, SortColumn::Date);
        return true;
    }
    if let Some(keys) = &keyboard.sort_size
        && keys.iter().any(|s| s == shortcut)
    {
        handle_sort(app, SortColumn::Size);
        return true;
    }
    false
}

fn handle_basic_nav(app: &mut AppState, code: KeyCode, modifiers: KeyModifiers) {
    if let (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) = (code, modifiers) {
        handle_type_char(app, c);
    } else {
        // Extract values needed from panel, then drop the borrow
        let (search_active, search_buffer_empty) = {
            let panel = app.active_tab_mut();
            (panel.is_search_active(), panel.search.buffer.is_empty())
        };

        match (code, modifiers) {
            (KeyCode::Tab, KeyModifiers::NONE) => handle_tab(app),
            (KeyCode::Up, _) => {
                if search_active {
                    handle_up_search(app);
                } else {
                    if !search_buffer_empty {
                        reset_search(app);
                    }
                    handle_up(app);
                }
            }
            (KeyCode::Down, _) => {
                if search_active {
                    handle_down_search(app);
                } else {
                    if !search_buffer_empty {
                        reset_search(app);
                    }
                    handle_down(app);
                }
            }
            (KeyCode::PageUp, _) => {
                reset_search(app);
                handle_page_up(app);
            }
            (KeyCode::PageDown, _) => {
                reset_search(app);
                handle_page_down(app);
            }
            (KeyCode::Home, _) => {
                reset_search(app);
                handle_home(app);
            }
            (KeyCode::End, _) => {
                reset_search(app);
                handle_end(app);
            }
            (KeyCode::Enter, _) => handle_open_item(app),
            (KeyCode::Esc, _) => {
                reset_search(app);
            }
            (KeyCode::Char(' '), KeyModifiers::NONE) => handle_toggle_selection(app),
            (KeyCode::Insert, _) => {
                handle_toggle_selection(app);
                handle_down(app);
            }
            _ => {}
        }
    }
}
