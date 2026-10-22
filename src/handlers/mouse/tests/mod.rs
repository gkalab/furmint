//! Test suite for `handlers::mouse`.
mod click;
mod drag;
mod popup;
mod scroll;
mod scrollbar;

use crate::app::{AppState, DragTarget};
use crate::app_state::tabs::PanelSide;
use crate::fs::utils::FileEntry;
use crate::handlers::mouse::scrollbar::{calculate_scroll_from_y, scrollbar_thumb_rows};
use crate::handlers::mouse::{
    handle_create_directory_popup_mouse, handle_create_file_popup_mouse, handle_mouse_event,
    handle_rename_popup_mouse, handle_viewer_search_popup_mouse, tick_viewer_autoscroll,
};
use crate::state::EmptyTrashState;
use crate::ui::text_edit::TextEditState;
use crate::ui::ui_utils::input_popup_text_area;
use ratatui::layout::Rect;
use std::path::PathBuf;
use termina::event::{Modifiers, MouseButton, MouseEvent, MouseEventKind};

fn setup_test_app() -> AppState {
    let mut app = AppState::test_default();
    app.file_viewer.is_visible = true;

    // Set panel areas so mouse clicks can be mapped
    app.layout.left_panel_area = Rect::new(0, 0, 40, 20);
    app.layout.right_panel_area = Rect::new(40, 0, 40, 20);
    app.layout.left_tab_bar_area = Rect::new(0, 0, 40, 1);

    // Setup some entries in left tab
    let entries = vec![
        FileEntry {
            name: "file1.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(100),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        },
        FileEntry {
            name: "file2.txt".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(200),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        },
    ];
    app.panels.left.active_tab_mut().entries = entries;
    app.panels.left.active_tab_mut().current_dir = PathBuf::from("/test");
    app.panels.left.active_tab_mut().cursor = 0;
    app.panels.left.active_tab_mut().recompute_visible_indices();

    app
}

fn file_entries(count: usize) -> Vec<FileEntry> {
    (0..count)
        .map(|i| FileEntry {
            name: format!("file_{i}.txt"),
            size: Some(100),
            modified: None,
            is_dir: false,
            is_symlink: false,
            attributes: String::new(),
            selected: false,
        })
        .collect()
}

/// Button areas of a confirmation popup of the given size on an 80x24 terminal.
/// Matches what the real draw code produces (see `draw_confirmation_popup`).
fn confirmation_button_areas(width: u16, height: u16) -> Vec<Rect> {
    crate::ui::ui_utils::confirmation_button_areas(width, height, Rect::new(0, 0, 80, 24))
}

/// Simulates the button layout used by the empty-trash confirmation popup
/// at terminal size 80x24. Popup is 60x7, centered; buttons "(N)o" and "(Y)es"
/// are drawn in the bottom row. These values must match what the real draw code produces.
fn empty_trash_button_areas() -> Vec<Rect> {
    confirmation_button_areas(60, 6)
}

/// An app whose viewer shows a small directory listing with icons enabled, so
/// clicks land on rows that render an icon before their name.
fn setup_viewer_listing_app() -> AppState {
    let mut app = AppState::test_default();
    app.file_viewer.is_visible = true;
    app.file_viewer.focused = true;
    app.global.icons = Some(true);
    app.global.borders = Some(false);
    app.layout.viewer.area = Rect::new(0, 0, 40, 20);
    app.file_viewer.text.content = vec!["/tmp/dir".to_string(), "alpha.txt".to_string()];
    app.file_viewer.text.directory_rows = Some(vec![
        crate::state::DirectoryListingRow {
            is_header: true,
            entry: listing_entry("/tmp/dir", true, false),
            hidden: false,
            executable: false,
            link_target: None,
        },
        crate::state::DirectoryListingRow {
            is_header: false,
            entry: listing_entry("alpha.txt", false, false),
            hidden: false,
            executable: false,
            link_target: None,
        },
    ]);
    app
}

/// A bare entry for a viewer listing row, with the metadata fields the draw pass
/// does not read left unset.
fn listing_entry(name: &str, is_dir: bool, is_symlink: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    }
}

fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: Modifiers::empty(),
    }
}

fn left_down(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: Modifiers::empty(),
    }
}

fn left_drag(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: x,
        row: y,
        modifiers: Modifiers::empty(),
    }
}

/// A viewer showing `lines` plain text lines, ready for selection drags.
///
/// The text area is 40 columns x 18 lines at y = 5 (no borders, no icons), so
/// the pointer can be dragged above the top edge as well as below the bottom.
fn setup_viewer_text_app(lines: usize) -> AppState {
    let mut app = AppState::test_default();
    app.file_viewer.is_visible = true;
    app.file_viewer.focused = true;
    app.global.icons = Some(false);
    app.global.borders = Some(false);
    app.layout.viewer.area = Rect::new(0, 5, 40, 20);
    app.file_viewer.text.content = (0..lines)
        .map(|i| format!("{i:03} abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ"))
        .collect();
    app
}

fn left_up(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: x,
        row: y,
        modifiers: Modifiers::empty(),
    }
}
