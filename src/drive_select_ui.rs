use crate::app::{AppState, PanelSide};
use crate::theme::ThemePalette;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem},
};
use termina::event::KeyCode;

#[cfg(windows)]
unsafe extern "system" {
    fn GetLogicalDrives() -> u32;
}

#[must_use]
pub fn get_available_drives() -> Vec<String> {
    #[cfg(windows)]
    {
        let mut drives = Vec::new();
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26 {
            if (mask >> i) & 1 == 1 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let drive_letter = (b'A' + i as u8) as char;
                drives.push(format!("{drive_letter}:\\"));
            }
        }
        drives
    }
    #[cfg(not(windows))]
    {
        // Fallback or empty for non-windows
        Vec::new()
    }
}

pub fn handle_drive_select_event(code: KeyCode, app: &mut AppState) -> bool {
    match code {
        KeyCode::Escape => {
            app.popups.drive_select.reset();
        }
        KeyCode::Down if !app.popups.drive_select.drives.is_empty() => {
            app.popups.drive_select.selected_index =
                (app.popups.drive_select.selected_index + 1) % app.popups.drive_select.drives.len();
        }
        KeyCode::Up if !app.popups.drive_select.drives.is_empty() => {
            if app.popups.drive_select.selected_index == 0 {
                app.popups.drive_select.selected_index = app.popups.drive_select.drives.len() - 1;
            } else {
                app.popups.drive_select.selected_index -= 1;
            }
        }
        KeyCode::Char(c) => {
            let target = format!("{}:\\", c.to_ascii_uppercase());
            if let Some(drive) = app
                .popups
                .drive_select
                .drives
                .iter()
                .find(|d| d.to_ascii_uppercase() == target)
                .cloned()
            {
                perform_drive_navigation(app, drive.as_str());
            }
        }
        KeyCode::Enter => {
            if let Some(drive) = app
                .popups
                .drive_select
                .drives
                .get(app.popups.drive_select.selected_index)
                .cloned()
            {
                perform_drive_navigation(app, &drive);
            }
        }
        _ => {}
    }
    false
}

fn perform_drive_navigation(app: &mut AppState, drive: &str) {
    let side = app.popups.drive_select.side;
    let mut path = std::path::PathBuf::from(drive);

    let get_drive = |p: &std::path::Path| -> Option<char> {
        p.components().find_map(|c| {
            if let std::path::Component::Prefix(prefix) = c {
                match prefix.kind() {
                    std::path::Prefix::Disk(d) | std::path::Prefix::VerbatimDisk(d) => {
                        Some((d as char).to_ascii_uppercase())
                    }
                    _ => None,
                }
            } else {
                None
            }
        })
    };

    if let Some(sel_drive) = get_drive(&path) {
        let (opp_drive, opp_dir) = {
            let opposite_tab = match side {
                PanelSide::Left => app.right.active_tab(),
                PanelSide::Right => app.left.active_tab(),
            };

            let opp_drive = if opposite_tab.provider.is_local() {
                get_drive(&opposite_tab.current_dir)
            } else {
                None
            };

            (opp_drive, opposite_tab.current_dir.clone())
        };

        if opp_drive == Some(sel_drive) {
            path = opp_dir;
        }
    }

    let success = {
        let tab_manager = match side {
            PanelSide::Left => &mut app.left,
            PanelSide::Right => &mut app.right,
        };
        tab_manager.active_tab_mut().navigate_to(&path).is_ok()
    };

    if success {
        app.popups.drive_select.reset();
        app.sync_watcher();
    }
}

pub fn draw_drive_select_popup(f: &mut Frame, app: &mut AppState, palette: &ThemePalette) {
    if !app.popups.drive_select.is_visible || app.popups.drive_select.drives.is_empty() {
        return;
    }

    let size = f.area();

    // Position the popup on the left or right side based on app.popups.drive_select.side
    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if app.popups.drive_select.side == PanelSide::Left {
            [
                Constraint::Percentage(5),  // Left margin
                Constraint::Percentage(30), // Popup width
                Constraint::Percentage(65), // Rest
            ]
        } else {
            [
                Constraint::Percentage(65), // Rest
                Constraint::Percentage(30), // Popup width
                Constraint::Percentage(5),  // Right margin
            ]
        })
        .split(size);

    let popup_column = horizontal_chunks[1];

    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Top margin (below tab bar/header)
            Constraint::Min(3),    // Popup height
            Constraint::Length(4), // Bottom margin
        ])
        .split(popup_column);

    // Adjust height based on number of drives
    let drive_count = u16::try_from(app.popups.drive_select.drives.len()).unwrap_or(u16::MAX);
    let height = (drive_count + 2).min(vertical_chunks[1].height);
    let popup_rect = Rect::new(
        vertical_chunks[1].x,
        vertical_chunks[1].y,
        vertical_chunks[1].width,
        height,
    );

    f.render_widget(Clear, popup_rect);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let list_bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let drive_selection_background =
        Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b);
    let drive_selection_foreground = if palette.is_dark {
        text_color
    } else {
        list_bg_color
    };

    let items: Vec<ListItem> = app
        .popups
        .drive_select
        .drives
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let is_selected = i == app.popups.drive_select.selected_index;
            if is_selected {
                ListItem::new(d.clone()).style(
                    Style::default()
                        .fg(drive_selection_foreground)
                        .bg(drive_selection_background)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ListItem::new(d.clone()).style(Style::default().fg(text_color))
            }
        })
        .collect();

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::EMPTY)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(bg_color)),
        popup_rect,
    );

    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(1)
        .vertical_margin(0)
        .constraints([Constraint::Min(1)])
        .split(popup_rect)[0];

    // Draw list inside
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::EMPTY)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(list_bg_color)),
    );

    f.render_widget(list, inner_area);
}
