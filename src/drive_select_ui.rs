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
extern "system" {
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
        KeyCode::Char('j') | KeyCode::Down => {
            if !app.drive_select_popup.drives.is_empty() {
                app.drive_select_popup.selected_index = (app.drive_select_popup.selected_index + 1)
                    % app.drive_select_popup.drives.len();
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if !app.drive_select_popup.drives.is_empty() {
                if app.drive_select_popup.selected_index == 0 {
                    app.drive_select_popup.selected_index = app.drive_select_popup.drives.len() - 1;
                } else {
                    app.drive_select_popup.selected_index -= 1;
                }
            }
        }
        KeyCode::Enter => {
            if let Some(drive) = app
                .drive_select_popup
                .drives
                .get(app.drive_select_popup.selected_index)
                .cloned()
            {
                let side = app.drive_select_popup.side;
                let path = std::path::PathBuf::from(&drive);

                // We must use a scope or temporary to avoid borrow checker issues if we mutations app further
                let success = {
                    let tab_manager = match side {
                        PanelSide::Left => &mut app.left,
                        PanelSide::Right => &mut app.right,
                    };
                    tab_manager.active_tab_mut().navigate_to(path).is_ok()
                };

                if success {
                    app.drive_select_popup.reset();
                    app.sync_watcher();
                }
            }
        }
        _ => {}
    }
    false
}

pub fn draw_drive_select_popup(f: &mut Frame, app: &mut AppState, _palette: &ThemePalette) {
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
    let drive_count = app.drive_select_popup.drives.len() as u16;
    let height = (drive_count + 2).min(vertical_chunks[1].height);
    let popup_rect = Rect::new(
        vertical_chunks[1].x,
        vertical_chunks[1].y,
        vertical_chunks[1].width,
        height,
    );

    f.render_widget(Clear, popup_rect);

    let items: Vec<ListItem> = app
        .drive_select_popup
        .drives
        .iter()
        .enumerate()
        .map(|(i, d)| {
            if i == app.drive_select_popup.selected_index {
                ListItem::new(format!("> {}", d)).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ListItem::new(format!("  {}", d)).style(Style::default().fg(Color::White))
            }
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Select Drive ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(list, popup_rect);
}
