use std::fs::{self, Metadata};
use std::path::{PathBuf};
use std::time::SystemTime;

use std::env;

use ratatui::prelude::*;
use ratatui::widgets::{Table, Row, Cell, Block, Borders};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use chrono::{DateTime, Local};


use anyhow::Result;
use catppuccin::PALETTE;

#[derive(Clone)]
struct FileEntry {
    name: String,
    is_dir: bool,
    size: Option<u64>,
    modified: Option<SystemTime>,
    attributes: String,
}

struct PanelState {
    current_dir: PathBuf,
    entries: Vec<FileEntry>,
    selected: usize,
    history: Vec<PathBuf>,
    history_index: usize,
    error: Option<String>,
}

#[derive(PartialEq)]
enum PanelSide {
    Left,
    Right,
}

struct AppState {
    left: PanelState,
    right: PanelState,
    active: PanelSide,
    status: Option<String>,
}

impl FileEntry {
    fn from_path(path: &PathBuf, meta: &Metadata) -> Self {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let is_dir = meta.is_dir();
        let size = if is_dir { None } else { Some(meta.len()) };
        let modified = meta.modified().ok();
        let attributes = get_attributes(meta, is_dir);
        FileEntry { name, is_dir, size, modified, attributes }
    }
}

fn get_attributes(meta: &Metadata, is_dir: bool) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        let mut attrs = String::new();
        attrs.push(if is_dir { 'd' } else { '-' });
        for i in (0..9).rev() {
            let bit = (mode >> i) & 1;
            attrs.push(match i % 3 {
                2 => if bit == 1 { 'r' } else { '-' },
                1 => if bit == 1 { 'w' } else { '-' },
                0 => if bit == 1 { 'x' } else { '-' },
                _ => '-',
            });
        }
        attrs
    }
    #[cfg(not(unix))]
    {
        if is_dir { "<DIR>".to_string() } else { "<FILE>".to_string() }
    }
}

fn list_dir(path: &PathBuf) -> Result<Vec<FileEntry>> {
    let mut entries = vec![];
    // Always add .. for going up
    entries.push(FileEntry {
        name: "..".to_string(),
        is_dir: true,
        size: None,
        modified: None,
        attributes: "".to_string(),
    });
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let file_path = entry.path();
        entries.push(FileEntry::from_path(&file_path, &meta));
    }
    // Sort: dirs first, then files, both alphabetically
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });
    Ok(entries)
}

use bytesize::ByteSize;

fn format_size(size: Option<u64>, is_dir: bool) -> String {
    // Use up to 1 decimal precision, units G/M/K, no space, pad <DIR> to 7 chars
    if is_dir {
        format!("{:>7}", "<DIR>")
    } else if let Some(s) = size {
        if s >= 1_000_000_000 {
            format!("{:>6.1}G", s as f64 / 1_000_000_000.0)
        } else if s >= 1_000_000 {
            format!("{:>6.1}M", s as f64 / 1_000_000.0)
        } else if s >= 1_000 {
            format!("{:>6.1}K", s as f64 / 1_000.0)
        } else {
            format!("{:>7}", s)
        }
    } else {
        "       ".to_string()
    }
}

fn format_modified(modified: Option<SystemTime>) -> String {
    if let Some(m) = modified {
        let dt: DateTime<Local> = m.into();
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        "".to_string()
    }
}

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    let palette = &PALETTE.macchiato;
    terminal.clear()?;

    let cwd = env::current_dir()?;
    let left_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        selected: 0,
        history: vec![cwd.clone()],
        history_index: 0,
        error: None,
    };
    let right_panel = PanelState {
        current_dir: cwd.clone(),
        entries: list_dir(&cwd).unwrap_or_default(),
        selected: 0,
        history: vec![cwd.clone()],
        history_index: 0,
        error: None,
    };
    let mut app = AppState {
        left: left_panel,
        right: right_panel,
        active: PanelSide::Left,
        status: None,
    };

    loop {
        terminal.draw(|f| {
            let size = f.size();
            let vertical_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1), // panels
                    Constraint::Length(1), // status lines
                ])
                .split(size);
            let panel_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(vertical_chunks[0]);
            let status_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(vertical_chunks[1]);
            draw_panel(f, &app.left, app.active == PanelSide::Left, panel_chunks[0], &palette);
            draw_panel(f, &app.right, app.active == PanelSide::Right, panel_chunks[1], &palette);
            draw_panel_status(f, &app.left, status_chunks[0], &palette, app.active == PanelSide::Left);
            draw_panel_status(f, &app.right, status_chunks[1], &palette, app.active == PanelSide::Right);
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(KeyEvent { code, modifiers, .. }) => {
                    match (code, modifiers) {
                        (KeyCode::Tab, _) => {
                            app.active = match app.active {
                                PanelSide::Left => PanelSide::Right,
                                PanelSide::Right => PanelSide::Left,
                            };
                        }
                        (KeyCode::Up, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if panel.selected > 0 {
                                panel.selected -= 1;
                            }
                        }
                        (KeyCode::Down, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if panel.selected + 1 < panel.entries.len() {
                                panel.selected += 1;
                            }
                        }
                        (KeyCode::Enter, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            let entry = &panel.entries[panel.selected];
                            if entry.is_dir {
                                let mut new_dir = panel.current_dir.clone();
                                if entry.name == ".." {
                                    if let Some(parent) = panel.current_dir.parent() {
                                        new_dir = parent.to_path_buf();
                                    }
                                } else {
                                    new_dir.push(&entry.name);
                                }
                                match list_dir(&new_dir) {
                                    Ok(entries) => {
                                        panel.current_dir = new_dir.clone();
                                        panel.entries = entries;
                                        panel.selected = 0;
                                        // Update history
                                        if panel.history_index + 1 < panel.history.len() {
                                            panel.history.truncate(panel.history_index + 1);
                                        }
                                        panel.history.push(new_dir);
                                        panel.history_index += 1;
                                        panel.error = None;
                                    }
                                    Err(e) => {
                                        panel.error = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        (KeyCode::Backspace, _) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if let Some(parent) = panel.current_dir.parent() {
                                let parent = parent.to_path_buf();
                                match list_dir(&parent) {
                                    Ok(entries) => {
                                        panel.current_dir = parent.clone();
                                        panel.entries = entries;
                                        panel.selected = 0;
                                        if panel.history_index + 1 < panel.history.len() {
                                            panel.history.truncate(panel.history_index + 1);
                                        }
                                        panel.history.push(parent);
                                        panel.history_index += 1;
                                        panel.error = None;
                                    }
                                    Err(e) => {
                                        panel.error = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        (KeyCode::Left, KeyModifiers::CONTROL) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if panel.history_index > 0 {
                                panel.history_index -= 1;
                                let dir = panel.history[panel.history_index].clone();
                                match list_dir(&dir) {
                                    Ok(entries) => {
                                        panel.current_dir = dir;
                                        panel.entries = entries;
                                        panel.selected = 0;
                                        panel.error = None;
                                    }
                                    Err(e) => {
                                        panel.error = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        (KeyCode::Right, KeyModifiers::CONTROL) => {
                            let panel = match app.active {
                                PanelSide::Left => &mut app.left,
                                PanelSide::Right => &mut app.right,
                            };
                            if panel.history_index + 1 < panel.history.len() {
                                panel.history_index += 1;
                                let dir = panel.history[panel.history_index].clone();
                                match list_dir(&dir) {
                                    Ok(entries) => {
                                        panel.current_dir = dir;
                                        panel.entries = entries;
                                        panel.selected = 0;
                                        panel.error = None;
                                    }
                                    Err(e) => {
                                        panel.error = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        (KeyCode::Char('q'), _) => {
                            break;
                        }
                        _ => {}
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    disable_raw_mode()?;
    Ok(())
}

use ratatui::widgets::TableState;

fn draw_panel(
    f: &mut ratatui::Frame,
    panel: &PanelState,
    active: bool,
    area: Rect,
    palette: &catppuccin::Flavor,
) {
    // Compute max width for Size column
    let size_width = panel.entries.iter()
        .map(|e| format_size(e.size, e.is_dir).len())
        .max()
        .unwrap_or(4);
    let size_header = format!("{:>width$}", "Size", width = size_width);
    let header = ["Name", &size_header, "Modified", "Attributes"];
    let rows = panel.entries.iter().map(|e| {
         Row::new(vec![
            Cell::from(e.name.clone()),
            Cell::from(format_size(e.size, e.is_dir)),
            Cell::from(format_modified(e.modified)),
            Cell::from(e.attributes.clone()),
        ])
    });
    let border_color = if active {
        Color::Rgb(
            palette.colors.blue.rgb.r,
            palette.colors.blue.rgb.g,
            palette.colors.blue.rgb.b,
        )
    } else {
        Color::Rgb(
            palette.colors.overlay0.rgb.r,
            palette.colors.overlay0.rgb.g,
            palette.colors.overlay0.rgb.b,
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel.current_dir.to_string_lossy())
        .border_style(Style::default().fg(border_color));
    let widths = [
        Constraint::Percentage(40),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
    ];
    let highlight_bg = if active {
        Color::Rgb(
            palette.colors.surface2.rgb.r,
            palette.colors.surface2.rgb.g,
            palette.colors.surface2.rgb.b,
        )
    } else {
        Color::Rgb(
            palette.colors.surface1.rgb.r,
            palette.colors.surface1.rgb.g,
            palette.colors.surface1.rgb.b,
        )
    };
    let highlight_fg = Color::Rgb(
        palette.colors.text.rgb.r,
        palette.colors.text.rgb.g,
        palette.colors.text.rgb.b,
    );
    let table = Table::new(rows, widths)
        .header(Row::new(header).style(Style::default().fg(Color::Rgb(
            palette.colors.yellow.rgb.r,
            palette.colors.yellow.rgb.g,
            palette.colors.yellow.rgb.b,
        ))))
        .block(block)
        .row_highlight_style(Style::default()
            .bg(highlight_bg)
            .fg(highlight_fg)
        );
    f.render_stateful_widget(table, area, &mut TableState::default().with_selected(Some(panel.selected)));
}

mod panel_status;
use panel_status::draw_panel_status;
