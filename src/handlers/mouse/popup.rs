//! Popup mouse routing: modal confirmations, list popups and input fields.

use crate::app::{AppState, DragTarget, PopupKind};
use termina::event::{KeyCode, Modifiers, MouseButton, MouseEvent, MouseEventKind};

use super::scroll::is_in_rect;
use super::scrollbar::{check_and_start_scrollbar_drag, update_drag_scroll};

/// Mouse handler for the help popup: the wheel scrolls the help table.
#[allow(clippy::unused_async)] // uniform handler signature, awaited by the registry
pub(crate) async fn handle_help_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    _is_double_click: bool,
) -> bool {
    match event.kind {
        MouseEventKind::ScrollUp => {
            crate::handlers::popup_help::handle_help_popup_event(KeyCode::Up, app);
            true
        }
        MouseEventKind::ScrollDown => {
            crate::handlers::popup_help::handle_help_popup_event(KeyCode::Down, app);
            true
        }
        _ => false,
    }
}

/// Mouse handler for the SSH connection popup: a confirmation overlay is a
/// modal button row, otherwise the popup's own fields and history list handle
/// the click.
#[allow(clippy::unused_async)] // uniform handler signature, awaited by the registry
pub(crate) async fn handle_ssh_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if app.popups.ssh_connection.confirmation.is_some() {
                handle_popup_down(app, event.column, event.row);
            } else {
                crate::handlers::popup_ssh::handle_ssh_connection_mouse_click(
                    app,
                    event.column,
                    event.row,
                    is_double_click,
                );
            }
            true
        }
        MouseEventKind::Drag(MouseButton::Left)
            if app.popups.ssh_connection.confirmation.is_none() =>
        {
            crate::handlers::popup_ssh::handle_ssh_field_drag(app, event.column, event.row);
            true
        }
        _ => false,
    }
}

/// Mouse handler for the SSH password popup: clicks set the caret, drags extend
/// the selection.
#[allow(clippy::unused_async)] // the popup mouse dispatch is always async
pub(crate) async fn handle_ssh_password_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            crate::handlers::popup_ssh::handle_ssh_password_mouse_click(
                app,
                event.column,
                event.row,
                is_double_click,
            );
            true
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            crate::handlers::popup_ssh::handle_ssh_password_field_drag(
                app,
                event.column,
                event.row,
            );
            true
        }
        _ => false,
    }
}

/// Routes a mouse event to the shared input-field hit-testing of the six
/// [`crate::ui::text_edit::TextEditState`] popups (Create File, Create
/// Directory, Rename, Rename Tab, Copy/Move and Viewer Search). A left click
/// inside the field sets the caret (or selects a word on double-click) and
/// starts a selection drag; a left drag extends the selection.
///
/// Returns whether the event was consumed. Clicks outside the field fall
/// through (via `false`) so the popup's own buttons still run — notably
/// Rename's overwrite confirmation.
fn route_input_popup_mouse(
    app: &mut AppState,
    kind: PopupKind,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    let (popup_width, state) = match kind {
        PopupKind::CreateFile => (60, &mut app.popups.create_file.input_value),
        PopupKind::CreateDirectory => (60, &mut app.popups.create_directory.new_name),
        PopupKind::Rename => {
            // The overwrite confirmation is a modal button row; leave its
            // clicks to the shared button hit-test.
            if app.popups.rename.show_overwrite_confirm {
                return false;
            }
            (60, &mut app.popups.rename.new_name)
        }
        PopupKind::RenameTab => (60, &mut app.popups.rename_tab.new_name),
        PopupKind::CopyMove => (76, &mut app.popups.copy_move.destination_input),
        PopupKind::ViewerSearch => (60, &mut app.popups.viewer_search.query),
        _ => return false,
    };

    let area = crate::ui::ui_utils::input_popup_text_area(popup_width, true, app.layout.screen);
    let column = usize::from(event.column.saturating_sub(area.x));
    let width = area.width as usize;

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if !is_in_rect((event.column, event.row), area) {
                return false;
            }
            if is_double_click {
                state.select_word_at(column, width);
            } else {
                let pos = state.cursor_from_column(column, width);
                state.set_cursor(pos);
            }
            app.mouse.active_drag = Some(DragTarget::InputFieldSelection);
            true
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let pos = state.cursor_from_column(column, width);
            state.extend_selection(pos);
            true
        }
        _ => false,
    }
}

/// Mouse handler for the create-file popup's text field.
#[allow(clippy::unused_async)] // uniform handler signature, awaited by the registry
pub(crate) async fn handle_create_file_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    route_input_popup_mouse(app, PopupKind::CreateFile, event, is_double_click)
}

/// Mouse handler for the create-directory popup's text field.
#[allow(clippy::unused_async)] // uniform handler signature, awaited by the registry
pub(crate) async fn handle_create_directory_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    route_input_popup_mouse(app, PopupKind::CreateDirectory, event, is_double_click)
}

/// Mouse handler for the rename popup's text field.
#[allow(clippy::unused_async)] // uniform handler signature, awaited by the registry
pub(crate) async fn handle_rename_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    route_input_popup_mouse(app, PopupKind::Rename, event, is_double_click)
}

/// Mouse handler for the rename-tab popup's text field.
#[allow(clippy::unused_async)] // uniform handler signature, awaited by the registry
pub(crate) async fn handle_rename_tab_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    route_input_popup_mouse(app, PopupKind::RenameTab, event, is_double_click)
}

/// Mouse handler for the copy/move popup's text field.
#[allow(clippy::unused_async)] // uniform handler signature, awaited by the registry
pub(crate) async fn handle_copy_move_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    route_input_popup_mouse(app, PopupKind::CopyMove, event, is_double_click)
}

/// Mouse handler for the viewer search popup's text field.
#[allow(clippy::unused_async)] // uniform handler signature, awaited by the registry
pub(crate) async fn handle_viewer_search_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    route_input_popup_mouse(app, PopupKind::ViewerSearch, event, is_double_click)
}

/// Mouse handler for the bookmark popup: clicks select a row (or a confirmation
/// button), and the wheel moves the selection.
pub(crate) async fn handle_bookmark_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) -> bool {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if app.popups.bookmark.confirmation.is_some() {
                handle_popup_down(app, event.column, event.row);
            } else {
                crate::handlers::popup_bookmark::handle_bookmark_mouse_click(
                    app,
                    event.column,
                    event.row,
                    is_double_click,
                )
                .await;
            }
            true
        }
        MouseEventKind::ScrollUp => {
            app.popups.bookmark.list.move_selection_up();
            true
        }
        MouseEventKind::ScrollDown => {
            app.popups.bookmark.list.move_selection_down();
            true
        }
        _ => false,
    }
}

pub(super) async fn handle_fuzzy_search_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if check_and_start_scrollbar_drag(app, event.column, event.row).await {
                return;
            }
            crate::handlers::popup_fuzzy::handle_fuzzy_search_mouse_click(
                app,
                event.column,
                event.row,
                is_double_click,
            )
            .await;
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.mouse.active_drag = None;
        }
        MouseEventKind::Drag(MouseButton::Left) if app.mouse.active_drag.is_some() => {
            update_drag_scroll(app, event.column, event.row).await;
        }
        MouseEventKind::ScrollUp => {
            app.fuzzy_search.move_selection_up();
        }
        MouseEventKind::ScrollDown => {
            app.fuzzy_search.move_selection_down();
        }
        _ => {}
    }
}

pub(super) async fn handle_popup_mouse(
    app: &mut AppState,
    event: MouseEvent,
    is_double_click: bool,
) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // An active confirmation overlay is modal: clicks and scrollbar
            // grabs on the popup behind it are ignored.
            let confirmation_overlay = (app.popups.ssh_connection.is_visible
                && app.popups.ssh_connection.confirmation.is_some())
                || (app.popups.bookmark.list.is_visible
                    && app.popups.bookmark.confirmation.is_some());

            if !confirmation_overlay
                && check_and_start_scrollbar_drag(app, event.column, event.row).await
            {
                return;
            }
            // Registered popups get first refusal (list rows, confirmations);
            // anything they leave falls through to the shared button hit-test.
            if app.route_popup_mouse(event, is_double_click).await {
                return;
            }
            handle_popup_down(app, event.column, event.row);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.mouse.active_drag = None;
            handle_popup_up(app, event.column, event.row).await;
        }
        MouseEventKind::Drag(MouseButton::Left) if app.mouse.active_drag.is_some() => {
            if matches!(
                app.mouse.active_drag,
                Some(DragTarget::SshFieldSelection | DragTarget::InputFieldSelection)
            ) {
                app.route_popup_mouse(event, is_double_click).await;
            } else {
                update_drag_scroll(app, event.column, event.row).await;
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            app.route_popup_mouse(event, is_double_click).await;
        }
        _ => {}
    }
}

/// The button popups in hit-test priority order (topmost first).
fn button_popups() -> &'static [PopupKind] {
    &[
        PopupKind::Error,
        PopupKind::Conflict,
        PopupKind::Rename,
        PopupKind::RemoteEdit,
        PopupKind::QuitConfirmation,
        PopupKind::Delete,
        PopupKind::EmptyTrash,
        PopupKind::HostKey,
        PopupKind::SshConnection,
        PopupKind::Bookmark,
    ]
}

/// Whether a popup is visible and its buttons should be hit-tested.
///
/// Rename only draws buttons in its overwrite-confirm variant; the SSH and
/// bookmark popups only when they have spawned a confirmation overlay.
fn is_button_popup_active(app: &AppState, kind: PopupKind) -> bool {
    let popups = &app.popups;
    match kind {
        PopupKind::Error => popups.error.is_visible,
        PopupKind::Conflict => popups.conflict.is_visible,
        PopupKind::Rename => popups.rename.is_visible && popups.rename.show_overwrite_confirm,
        PopupKind::RemoteEdit => popups.remote_edit.is_visible,
        PopupKind::QuitConfirmation => popups.quit_confirmation.is_visible,
        PopupKind::Delete => popups.delete.is_visible,
        PopupKind::EmptyTrash => popups.empty_trash.is_visible,
        PopupKind::HostKey => popups.host_key.is_visible,
        PopupKind::SshConnection => {
            popups.ssh_connection.is_visible && popups.ssh_connection.confirmation.is_some()
        }
        PopupKind::Bookmark => {
            popups.bookmark.list.is_visible && popups.bookmark.confirmation.is_some()
        }
        _ => false,
    }
}

/// Applies the visual feedback (focus/selection) for the pressed button.
fn set_focused_button(app: &mut AppState, kind: PopupKind, index: usize) {
    let popups = &mut app.popups;
    match kind {
        PopupKind::Error => popups.error.focused_button = index,
        PopupKind::Conflict => popups.conflict.focused_button = index,
        PopupKind::Rename => popups.rename.focused_button = index,
        PopupKind::RemoteEdit => popups.remote_edit.focused_button = index,
        PopupKind::QuitConfirmation => popups.quit_confirmation.selected_no = index == 0,
        PopupKind::Delete => popups.delete.selected_no = index == 0,
        PopupKind::EmptyTrash => popups.empty_trash.selected_no = index == 0,
        PopupKind::HostKey => popups.host_key.selected_no = index == 0,
        PopupKind::SshConnection => {
            if let Some(confirmation) = popups.ssh_connection.confirmation.as_mut() {
                confirmation.selected_no = index == 0;
            }
        }
        PopupKind::Bookmark => {
            if let Some(confirmation) = popups.bookmark.confirmation.as_mut() {
                confirmation.selected_no = index == 0;
            }
        }
        _ => {}
    }
}

/// Returns the index of the popup button under the cursor, or `None` when no
/// active button popup covers the position.
///
/// Stops at the topmost active popup, so a click that misses its buttons does
/// not fall through to popups underneath.
fn find_button_index(app: &AppState, x: u16, y: u16) -> Option<usize> {
    let pos = (x, y);
    button_popups()
        .iter()
        .find_map(|&kind| {
            if is_button_popup_active(app, kind) {
                Some(
                    app.layout
                        .popups
                        .button_areas(kind)
                        .iter()
                        .position(|rect| is_in_rect(pos, *rect)),
                )
            } else {
                None
            }
        })
        .flatten()
}

/// The topmost active button popup, if any.
fn active_button_popup(app: &AppState) -> Option<PopupKind> {
    button_popups()
        .iter()
        .copied()
        .find(|&kind| is_button_popup_active(app, kind))
}

pub(crate) fn handle_popup_down(app: &mut AppState, x: u16, y: u16) {
    if let Some(i) = find_button_index(app, x, y) {
        app.mouse.mouse_button_down_index = Some(i);

        // Set the focused button for visual feedback
        if let Some(kind) = active_button_popup(app) {
            set_focused_button(app, kind, i);
        }
    }
}

async fn handle_popup_up(app: &mut AppState, x: u16, y: u16) {
    let down_index = app.mouse.mouse_button_down_index.take();
    let Some(down_index) = down_index else { return };

    let current_index = find_button_index(app, x, y);
    if current_index != Some(down_index) {
        return;
    }

    // Button click confirmed — apply visual feedback, then hand the press to
    // the popup's registered key handler as an Enter.
    let Some(kind) = active_button_popup(app) else {
        return;
    };
    set_focused_button(app, kind, down_index);
    kind.dispatch_key(app, KeyCode::Enter, Modifiers::NONE)
        .await;
}
