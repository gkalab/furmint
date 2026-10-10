use crate::app::AppState;
use crate::app_state::tabs::PanelSide;
use crate::config::KeyboardConfig;
use crate::theme::ThemePalette;
use crate::ui::{draw_panel, draw_panel_status};
use ratatui::prelude::*;

pub fn draw_main_layout(f: &mut Frame, app: &mut AppState, palette: &ThemePalette) {
    // All geometry comes from `app.layout`, which `event_loop::draw_ui` computed
    // before this pass. Nothing here re-derives layout.
    let size = f.area();

    // Fill background
    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let bg = ratatui::widgets::Paragraph::new("").style(Style::default().bg(bg_color));
    f.render_widget(bg, size);

    // Left Panel
    draw_side(f, app, PanelSide::Left, palette);

    // Right Panel
    draw_side(f, app, PanelSide::Right, palette);

    // Status Bars
    draw_status_bars(f, app, palette);
}

fn draw_side(f: &mut Frame, app: &mut AppState, side: PanelSide, palette: &ThemePalette) {
    let opposite_side = side.opposite();
    let is_viewer_visible = app.file_viewer.is_visible && app.panels.active == opposite_side;

    let tab_bar_area = *app.layout.tab_bar_area(side);
    let panel_area = *app.layout.panel_area(side);
    let show_tabs = app.layout.show_tabs;

    if is_viewer_visible {
        crate::ui::draw_file_viewer(
            f,
            &mut app.file_viewer,
            panel_area,
            &app.layout.viewer,
            palette,
            app.global.borders.unwrap_or(false),
            app.global.icons.unwrap_or(false),
        );
        return;
    }

    let is_active = app.panels.active == side && !app.file_viewer.focused;
    let tab_manager = if side == PanelSide::Left {
        &app.panels.left
    } else {
        &app.panels.right
    };

    if show_tabs {
        crate::ui::draw_tab_bar(
            f,
            tab_manager,
            tab_bar_area,
            palette,
            is_active,
            app.global.borders.unwrap_or(false),
            app.global.icons.unwrap_or(false),
        );
    }

    let tab = if side == PanelSide::Left {
        app.panels.left.active_tab_mut()
    } else {
        app.panels.right.active_tab_mut()
    };
    draw_panel(
        f,
        tab,
        is_active,
        panel_area,
        palette,
        app.global.borders.unwrap_or(false),
        app.global.icons.unwrap_or(false),
    );
}

fn draw_status_bars(f: &mut Frame, app: &AppState, palette: &ThemePalette) {
    for (side, tab, active) in [
        (
            PanelSide::Left,
            app.panels.left.active_tab(),
            app.panels.active == PanelSide::Left,
        ),
        (
            PanelSide::Right,
            app.panels.right.active_tab(),
            app.panels.active == PanelSide::Right,
        ),
    ] {
        draw_panel_status(
            f,
            tab,
            *app.layout.status_area(side),
            &crate::ui::panel::PanelStatusContext {
                palette,
                active,
                borders: app.global.borders.unwrap_or(false),
                task_manager: &app.tasks.task_manager,
                side,
            },
        );
    }
}

pub fn draw_all_popups(
    f: &mut Frame,
    app: &mut AppState,
    palette: &ThemePalette,
    keyboard: &KeyboardConfig,
) {
    // Draw popups in order of layering
    crate::ui::fuzzy_search_ui::draw_fuzzy_search_popup(
        f,
        &mut app.fuzzy_search,
        app.layout.popups.fuzzy_search.list_area,
        palette,
    );
    crate::ui::rename_ui::draw_rename_popup(
        f,
        &app.popups.rename,
        &app.layout.popups.rename,
        palette,
    );
    crate::ui::viewer::draw_viewer_search_popup(f, &app.popups.viewer_search, palette);
    crate::ui::rename_tab_ui::draw_rename_tab_popup(f, &app.popups.rename_tab, palette);
    crate::ui::create_dir_ui::draw_create_dir_popup(f, &app.popups.create_directory, palette);
    crate::ui::create_file_ui::draw_create_file_popup(f, &app.popups.create_file, palette);
    crate::ui::delete_ui::draw_delete_popup(
        f,
        &app.popups.delete,
        &app.layout.popups.delete,
        palette,
    );
    crate::ui::copy_move_ui::draw_copy_move_popup(f, &app.popups.copy_move, palette);
    crate::ui::conflict_ui::draw_conflict_popup(
        f,
        &app.popups.conflict,
        &app.layout.popups.conflict,
        palette,
    );
    crate::ui::task_ui::draw_task_manager(
        f,
        &app.tasks.task_manager,
        app.tasks.show_task_manager,
        palette,
    );
    crate::ui::empty_trash_ui::draw_empty_trash_popup(
        f,
        &app.popups.empty_trash,
        &app.layout.popups.empty_trash,
        palette,
    );
    crate::ui::quit_ui::draw_quit_popup(
        f,
        &app.popups.quit_confirmation,
        &app.layout.popups.quit_confirmation,
        palette,
    );
    crate::ui::error_ui::draw_error_popup(f, &app.popups.error, &app.layout.popups.error, palette);
    crate::ui::help_ui::draw_help_popup(f, app, keyboard, palette);

    if app.popups.drive_select.is_visible {
        crate::ui::drive_select_ui::draw_drive_select_popup(f, app, palette);
    }
    if app.popups.ssh_connection.is_visible {
        crate::ui::ssh_ui::draw_ssh_connection_popup(f, app, palette);
    }
    if app.popups.ssh_password.is_visible {
        crate::ui::ssh_ui::draw_ssh_password_popup(f, app, palette);
    }
    crate::ui::host_key_ui::draw_host_key_popup(
        f,
        &app.popups.host_key,
        &app.layout.popups.host_key,
        palette,
    );
    crate::ui::remote_edit_ui::draw_remote_edit_popup(
        f,
        &app.popups.remote_edit,
        &app.layout.popups.remote_edit,
        palette,
    );
    crate::ui::bookmark_ui::draw_bookmark_popup(
        f,
        &mut app.popups.bookmark,
        &app.layout.popups.bookmark,
        palette,
    );
}
