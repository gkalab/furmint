use crate::app::{AppState, PanelSide};
use crate::config::KeyboardConfig;
use crate::theme::ThemePalette;
use crate::ui::{draw_panel, draw_panel_status};
use ratatui::prelude::*;

pub fn draw_main_layout(f: &mut Frame, app: &mut AppState, palette: &ThemePalette) {
    let size = f.area();

    // Fill background
    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let bg = ratatui::widgets::Paragraph::new("").style(Style::default().bg(bg_color));
    f.render_widget(bg, size);

    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // panels
            Constraint::Length(1), // status lines
        ])
        .split(size);

    let panel_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(vertical_chunks[0]);

    let status_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(vertical_chunks[1]);

    let show_tabs = app.left.tabs.len() > 1 || app.right.tabs.len() > 1;

    // Left Panel
    draw_side(f, app, PanelSide::Left, panel_chunks[0], palette, show_tabs);

    // Right Panel
    draw_side(
        f,
        app,
        PanelSide::Right,
        panel_chunks[1],
        palette,
        show_tabs,
    );

    // Status Bars
    draw_status_bars(f, app, &status_chunks, palette);
}

fn draw_side(
    f: &mut Frame,
    app: &mut AppState,
    side: PanelSide,
    area: Rect,
    palette: &ThemePalette,
    show_tabs: bool,
) {
    let opposite_side = if side == PanelSide::Left {
        PanelSide::Right
    } else {
        PanelSide::Left
    };
    let is_viewer_visible = app.file_viewer.is_visible && app.active == opposite_side;

    if is_viewer_visible {
        app.file_viewer.area = area;
        crate::ui::draw_file_viewer(
            f,
            &mut app.file_viewer,
            area,
            palette,
            app.global.borders.unwrap_or(false),
        );
    } else {
        let is_active = app.active == side && !app.file_viewer.focused;
        let tab_manager = if side == PanelSide::Left {
            &app.left
        } else {
            &app.right
        };

        let sub_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                if show_tabs {
                    Constraint::Length(1)
                } else {
                    Constraint::Length(0)
                },
                Constraint::Min(1),
            ])
            .split(area);

        if show_tabs {
            if side == PanelSide::Left {
                app.left_tab_bar_area = sub_layout[0];
            } else {
                app.right_tab_bar_area = sub_layout[0];
            }
            let tab_areas = crate::ui::draw_tab_bar(
                f,
                tab_manager,
                sub_layout[0],
                palette,
                is_active,
                app.global.borders.unwrap_or(false),
                app.global.icons.unwrap_or(false),
            );
            if side == PanelSide::Left {
                app.left_tab_areas = tab_areas;
            } else {
                app.right_tab_areas = tab_areas;
            }
        } else if side == PanelSide::Left {
            app.left_tab_bar_area = Rect::default();
        } else {
            app.right_tab_bar_area = Rect::default();
        }

        if side == PanelSide::Left {
            app.left_panel_area = sub_layout[1];
        } else {
            app.right_panel_area = sub_layout[1];
        }

        let tab = if side == PanelSide::Left {
            app.left.active_tab_mut()
        } else {
            app.right.active_tab_mut()
        };
        draw_panel(
            f,
            tab,
            is_active,
            sub_layout[1],
            palette,
            app.global.borders.unwrap_or(false),
            app.global.icons.unwrap_or(false),
        );
    }
}

fn draw_status_bars(f: &mut Frame, app: &AppState, chunks: &[Rect], palette: &ThemePalette) {
    draw_panel_status(
        f,
        app.left.active_tab(),
        chunks[0],
        &crate::ui::panel::PanelStatusContext {
            palette,
            active: app.active == PanelSide::Left,
            borders: app.global.borders.unwrap_or(false),
            task_manager: &app.task_manager,
            side: PanelSide::Left,
        },
    );
    draw_panel_status(
        f,
        app.right.active_tab(),
        chunks[1],
        &crate::ui::panel::PanelStatusContext {
            palette,
            active: app.active == PanelSide::Right,
            borders: app.global.borders.unwrap_or(false),
            task_manager: &app.task_manager,
            side: PanelSide::Right,
        },
    );
}

pub fn draw_all_popups(
    f: &mut Frame,
    app: &mut AppState,
    palette: &ThemePalette,
    keyboard: &KeyboardConfig,
) {
    // Draw popups in order of layering
    crate::ui::fuzzy_search_ui::draw_fuzzy_search_popup(f, &mut app.fuzzy_search, palette);
    crate::ui::rename_ui::draw_rename_popup(f, &app.popups.rename, palette);
    crate::ui::rename_tab_ui::draw_rename_tab_popup(f, &app.popups.rename_tab, palette);
    crate::ui::create_dir_ui::draw_create_dir_popup(f, &app.popups.create_directory, palette);
    crate::ui::create_file_ui::draw_create_file_popup(f, &app.popups.create_file, palette);
    crate::ui::delete_ui::draw_delete_popup(f, &app.popups.delete, palette);
    crate::ui::copy_move_ui::draw_copy_move_popup(f, &app.popups.copy_move, palette);
    crate::ui::conflict_ui::draw_conflict_popup(f, &app.popups.conflict, palette);
    crate::ui::task_ui::draw_task_manager(f, &app.task_manager, app.show_task_manager, palette);
    crate::ui::empty_trash_ui::draw_empty_trash_popup(f, &app.popups.empty_trash, palette);
    crate::ui::quit_ui::draw_quit_popup(f, &app.popups.quit_confirmation, palette);
    crate::ui::error_ui::draw_error_popup(f, &app.popups.error, palette);
    crate::ui::help_ui::draw_help_popup(f, app, keyboard, palette);

    if app.popups.drive_select.is_visible {
        crate::drive_select_ui::draw_drive_select_popup(f, app, palette);
    }
    if app.popups.ssh_connection.is_visible {
        crate::ui::ssh_ui::draw_ssh_connection_popup(f, app, palette);
    }
    if app.popups.ssh_password.is_visible {
        crate::ui::ssh_ui::draw_ssh_password_popup(f, app, palette);
    }
    crate::ui::remote_edit_ui::draw_remote_edit_popup(f, &app.popups.remote_edit, palette);
}
