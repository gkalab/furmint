use crate::app::{AppState, PanelSide};
use crate::theme::ThemePalette;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem},
};

#[cfg(windows)]
unsafe extern "system" {
    fn GetLogicalDrives() -> u32;
}

pub fn get_available_drives() -> Vec<String> {
    #[cfg(windows)]
    {
        let mut drives = Vec::new();
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26 {
            if (mask >> i) & 1 == 1 {
                let drive_letter = (b'A' + i as u8) as char;
                drives.push(format!("{}:\\", drive_letter));
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
        KeyCode::Esc => {
            app.drive_select_popup.reset();
        }
        KeyCode::Down => {
            if !app.drive_select_popup.drives.is_empty() {
                app.drive_select_popup.selected_index = (app.drive_select_popup.selected_index + 1)
                    % app.drive_select_popup.drives.len();
            }
        }
        KeyCode::Up => {
            if !app.drive_select_popup.drives.is_empty() {
                if app.drive_select_popup.selected_index == 0 {
                    app.drive_select_popup.selected_index = app.drive_select_popup.drives.len() - 1;
                } else {
                    app.drive_select_popup.selected_index -= 1;
                }
            }
        }
        KeyCode::Char(c) => {
            let target = format!("{}:\\", c.to_ascii_uppercase());
            if let Some(drive) = app
                .drive_select_popup
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
                .drive_select_popup
                .drives
                .get(app.drive_select_popup.selected_index)
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
    let side = app.drive_select_popup.side;
    let path = std::path::PathBuf::from(drive);

    let success = {
        let tab_manager = match side {
            PanelSide::Left => &mut app.left,
            PanelSide::Right => &mut app.right,
        };
        tab_manager.active_tab_mut().navigate_to(&path).is_ok()
    };

    if success {
        app.drive_select_popup.reset();
        app.sync_watcher();
    }
}

pub fn draw_drive_select_popup(f: &mut Frame, app: &mut AppState, palette: &ThemePalette) {
    if !app.drive_select_popup.is_visible || app.drive_select_popup.drives.is_empty() {
        return;
    }

    let size = f.area();

    // Position the popup on the left or right side based on app.drive_select_popup.side
    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if app.drive_select_popup.side == PanelSide::Left {
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
    let drive_count = u16::try_from(app.drive_select_popup.drives.len()).unwrap_or(u16::MAX);
    let height = (drive_count + 2).min(vertical_chunks[1].height);
    let popup_rect = Rect::new(
        vertical_chunks[1].x,
        vertical_chunks[1].y,
        vertical_chunks[1].width,
        height,
    );

    f.render_widget(Clear, popup_rect);

    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let drive_selection_background =
        Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b);
    let drive_selection_foreground = if palette.is_dark {
        text_color
    } else {
        bg_color
    };

    let items: Vec<ListItem> = app
        .drive_select_popup
        .drives
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let is_selected = i == app.drive_select_popup.selected_index;
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

    let list = List::new(items).block(
        Block::default()
            .title(" Select Drive ")
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(bg_color)),
    );

    f.render_widget(list, popup_rect);
}
