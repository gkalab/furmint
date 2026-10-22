use crate::app::AppState;
use crate::app_state::tabs::PanelSide;
use crate::state::Drive;
use crate::theme::ThemePalette;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Text,
    widgets::{Block, Borders, Cell, Clear, Row, Table},
};
#[cfg(target_os = "linux")]
use std::ffi::CString;

#[cfg(windows)]
unsafe extern "system" {
    fn GetLogicalDrives() -> u32;
}

/// Filesystem types that are kernel-provided or virtual rather than backed by
/// a real volume. Mounts with these types are never offered as drives.
#[cfg(target_os = "linux")]
const VIRTUAL_FS_TYPES: &[&str] = &[
    "autofs",
    "bpf",
    "binfmt_misc",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devfs",
    "devpts",
    "devtmpfs",
    "efivarfs",
    "fusectl",
    "fuse.portal",
    "hugetlbfs",
    "mqueue",
    "nsfs",
    "pstore",
    "proc",
    "procfs",
    "ramfs",
    "rpc_pipefs",
    "securityfs",
    "selinuxfs",
    "sunrpc",
    "sysfs",
    "tmpfs",
    "tracefs",
];

/// Undoes the octal escaping `/proc/mounts` applies to special characters
/// in mount points (`\040` space, `\011` tab, `\012` newline, `\134` backslash).
#[cfg(target_os = "linux")]
fn unescape_mount_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            match &bytes[i + 1..i + 4] {
                b"040" => out.push(b' '),
                b"011" => out.push(b'\t'),
                b"012" => out.push(b'\n'),
                b"134" => out.push(b'\\'),
                _ => out.push(bytes[i]),
            }
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Returns `(total_bytes, free_bytes, used_pct)` for the mount at `path`.
#[cfg(target_os = "linux")]
fn mount_capacity(path: &std::path::Path) -> Option<(u64, u64, u64)> {
    let s = path.to_str()?;
    let Ok(c_path) = CString::new(s) else {
        return None;
    };
    let mut sv: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), std::ptr::from_mut(&mut sv)) } != 0 {
        return None;
    }
    let frsize = u128::from(sv.f_frsize);
    let total = frsize * u128::from(sv.f_blocks);
    let free = frsize * u128::from(sv.f_bavail);
    let used_pct = (sv.f_blocks.saturating_sub(sv.f_bfree) * 100)
        .checked_div(sv.f_blocks)
        .unwrap_or(0);
    Some((
        total.try_into().unwrap_or(u64::MAX),
        free.try_into().unwrap_or(u64::MAX),
        used_pct,
    ))
}

/// Formats mount capacity into the full ("930.4G, 146.2G free, 84% used")
/// and compact ("930.4G/146.2G/84%") detail strings.
fn format_drive_detail(total: &str, free: &str, used_pct: u64) -> (String, String) {
    (
        format!("{total}, {free} free, {used_pct}% used"),
        format!("{total}/{free}/{used_pct}%"),
    )
}

/// Lists the real mounts from `/proc/mounts`, skipping virtual filesystems.
#[cfg(target_os = "linux")]
fn linux_available_drives() -> Vec<Drive> {
    let Ok(content) = std::fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };

    let mut mounts: Vec<(String, (u64, u64, u64))> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let _source = fields.next();
        let Some(mount) = fields.next().and_then(unescape_mount_component) else {
            continue;
        };
        let Some(fstype) = fields.next() else {
            continue;
        };
        if VIRTUAL_FS_TYPES.contains(&fstype) {
            continue;
        }
        let mount_path = std::path::Path::new(&mount);
        if ["/sys", "/proc", "/dev", "/run"]
            .iter()
            .any(|prefix| mount_path.starts_with(prefix))
        {
            continue;
        }
        if !seen.insert(mount.clone()) {
            continue;
        }
        let Some(capacity) = mount_capacity(std::path::Path::new(&mount)) else {
            continue;
        };
        mounts.push((mount, capacity));
    }

    mounts
        .into_iter()
        .map(|(mount, (total, free, used_pct))| {
            let total = crate::fs::utils::format_size(Some(total), false, false);
            let free = crate::fs::utils::format_size(Some(free), false, false);
            let total = total.trim();
            let free = free.trim();
            let (detail, detail_short) = format_drive_detail(total, free, used_pct);
            Drive {
                path: mount,
                detail,
                detail_short,
            }
        })
        .collect()
}

/// Returns `(total_bytes, free_bytes, used_pct)` for the drive at `path`.
#[cfg(windows)]
fn drive_capacity(path: &str) -> Option<(u64, u64, u64)> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::HSTRING;

    let wide = HSTRING::from(path);
    let mut free = 0u64;
    let mut total = 0u64;
    let mut total_free = 0u64;
    let free_ptr = std::ptr::addr_of_mut!(free);
    let total_ptr = std::ptr::addr_of_mut!(total);
    let total_free_ptr = std::ptr::addr_of_mut!(total_free);
    unsafe { GetDiskFreeSpaceExW(&wide, Some(free_ptr), Some(total_ptr), Some(total_free_ptr)) }
        .ok()?;
    let used_pct = total.saturating_sub(total_free) * 100 / total.max(1);
    Some((total, free, used_pct))
}

#[must_use]
pub fn get_available_drives() -> Vec<Drive> {
    #[cfg(windows)]
    {
        let mut drives = Vec::new();
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26 {
            if (mask >> i) & 1 == 1 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let drive_letter = (b'A' + i as u8) as char;
                let path = format!("{drive_letter}:\\");
                let (detail, detail_short) = drive_capacity(&path)
                    .map(|(total, free, used_pct)| {
                        let total = crate::fs::utils::format_size(Some(total), false, false);
                        let free = crate::fs::utils::format_size(Some(free), false, false);
                        format_drive_detail(total.trim(), free.trim(), used_pct)
                    })
                    .unwrap_or_default();
                drives.push(Drive {
                    path,
                    detail,
                    detail_short,
                });
            }
        }
        drives
    }
    #[cfg(target_os = "linux")]
    {
        linux_available_drives()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Vec::new()
    }
}

/// Builds a table row per drive, right-aligning the detail. When the full
/// detail would not fit in `detail_width` columns and a compact form exists,
/// the compact form is used instead.
fn build_drive_rows<'a>(
    app: &'a AppState,
    detail_width: u16,
    palette: &'a ThemePalette,
) -> Vec<Row<'a>> {
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let selection_background =
        Color::Rgb(palette.surface2.r, palette.surface2.g, palette.surface2.b);
    let selection_foreground = if palette.is_dark {
        text_color
    } else {
        Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b)
    };

    app.popups
        .drive_select
        .drives
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let style = if i == app.popups.drive_select.selected_index {
                Style::default()
                    .fg(selection_foreground)
                    .bg(selection_background)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(text_color)
            };
            let fits_full =
                u16::try_from(d.detail.chars().count()).is_ok_and(|n| n <= detail_width);
            let detail = if fits_full || d.detail_short.is_empty() {
                d.detail.as_str()
            } else {
                d.detail_short.as_str()
            };
            Row::new([
                Cell::new(d.path.clone()),
                Cell::new(Text::from(detail).alignment(Alignment::Right)),
            ])
            .style(style)
        })
        .collect()
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

    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(1)
        .vertical_margin(0)
        .constraints([Constraint::Min(1)])
        .split(popup_rect)[0];

    // Width of the detail column (70% of the table area, inside its border),
    // used to decide between the full and the compact detail form.
    let detail_width =
        u16::try_from(u32::from(inner_area.width.saturating_sub(2)) * 70 / 100).unwrap_or(u16::MAX);

    let rows = build_drive_rows(app, detail_width, palette);

    // Draw outer block
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::EMPTY)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(bg_color)),
        popup_rect,
    );

    // Draw table inside: path on the left, detail right-aligned.
    let table = Table::new(
        rows,
        [Constraint::Percentage(30), Constraint::Percentage(70)],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::EMPTY)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(list_bg_color)),
    );

    f.render_widget(table, inner_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn test_unescape_mount_component() {
        assert_eq!(unescape_mount_component("home").unwrap(), "home");
        // \040 = space, \134 = backslash, \011 = tab
        assert_eq!(unescape_mount_component("my\\040dir").unwrap(), "my dir");
        assert_eq!(unescape_mount_component("a\\134b").unwrap(), "a\\b");
        assert_eq!(unescape_mount_component("a\\011b").unwrap(), "a\tb");
    }

    #[test]
    fn test_drive_detail_formats() {
        let (full, short) = format_drive_detail("930.4G", "146.2G", 84);
        assert_eq!(full, "930.4G, 146.2G free, 84% used");
        assert_eq!(short, "930.4G/146.2G/84%");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_root_is_offered_and_virtual_mounts_are_skipped() {
        let drives = get_available_drives();
        assert!(
            drives.iter().any(|d| d.path == "/"),
            "root mount should always be offered, got {drives:?}"
        );
        for d in &drives {
            // Kernel/system pseudo-mounts must never appear.
            assert!(
                !["/sys", "/proc", "/dev", "/run"]
                    .iter()
                    .any(|prefix| std::path::Path::new(&d.path).starts_with(prefix)),
                "unexpected system mount: {}",
                d.path
            );
        }
    }
}
