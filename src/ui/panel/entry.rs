use crate::app_state::tabs::{SortColumn, SortDirection};
use crate::fs::utils::{FileEntry, format_modified, format_size};
use crate::theme::{Rgb, ThemePalette};
use crate::ui::ui_utils::truncate_middle_with_ellipsis;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Row};
use std::collections::HashMap;
use std::path::Path;

/// Context for rendering an entry row
pub(super) struct EntryRowContext<'a> {
    pub(super) palette: &'a ThemePalette,
    pub(super) icons_enabled: bool,
    pub(super) current_dir: &'a Path,
    pub(super) name_col_width: usize,
    /// Cached directory sizes: path -> size in bytes
    pub(super) dir_sizes: &'a std::collections::HashMap<std::path::PathBuf, u64>,
    /// True when `entry.attributes` holds Unix-style permissions
    /// (`drwxr-xr-x`) rather than the short Windows flag format.
    pub(super) unix_attributes: bool,
}

pub(super) fn sort_indicator(
    column: SortColumn,
    current_column: SortColumn,
    direction: SortDirection,
) -> &'static str {
    if column == current_column {
        match direction {
            SortDirection::Ascending => "▴",
            SortDirection::Descending => "▾",
        }
    } else {
        ""
    }
}

/// Get style for a file entry based on its type.
///
/// `unix_attributes` is [`crate::fs::utils::has_unix_attributes`] for the
/// browsed filesystem, so a remote tab mutes the remote host's dotfiles from
/// any platform.
pub(super) fn style_for_entry(
    entry: &FileEntry,
    current_dir: &Path,
    palette: &ThemePalette,
    unix_attributes: bool,
) -> Style {
    entry_foreground(
        entry.is_dir,
        entry.is_symlink,
        entry.is_hidden(unix_attributes),
        crate::fs::utils::is_executable(&current_dir.join(&entry.name), entry),
        palette,
    )
}

/// Foreground color for a listing entry whose type flags the caller has already
/// resolved. Shared with the viewer's directory preview so both listings color
/// entries identically.
///
/// Symlinks keep their own cyan regardless of the target. Everything else that
/// is hidden (a leading dot on a Unix filesystem, `FILE_ATTRIBUTE_HIDDEN` on a
/// local Windows one) is de-emphasized with the muted `subtext` color, whether
/// it is a file or a directory.
#[allow(clippy::fn_params_excessive_bools)] // the flags are independent entry-type attributes
pub(crate) fn entry_foreground(
    is_dir: bool,
    is_symlink: bool,
    hidden: bool,
    executable: bool,
    palette: &ThemePalette,
) -> Style {
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
    if is_symlink {
        return Style::default().fg(rgb(palette.cyan));
    }
    if hidden {
        return Style::default().fg(rgb(palette.subtext));
    }
    if is_dir {
        return Style::default().fg(rgb(palette.blue));
    }
    if executable {
        return Style::default().fg(rgb(palette.green));
    }
    Style::default().fg(rgb(palette.text))
}

/// Style of the icon drawn in front of an entry name.
pub(super) fn style_for_icon(
    entry: &FileEntry,
    palette: &ThemePalette,
    name_style: Style,
    unix_attributes: bool,
) -> Style {
    entry_icon_foreground(
        entry.is_dir,
        entry.is_symlink,
        entry.is_hidden(unix_attributes),
        name_style,
        palette,
    )
}

/// Style of the icon drawn in front of an entry name, given type flags the
/// caller has already resolved.
///
/// Only hidden directories deviate from their name color: their icon keeps the
/// standard directory blue so the row still reads as a directory, while the
/// muted name carries the "hidden" signal. Every other entry matches its name.
pub(crate) fn entry_icon_foreground(
    is_dir: bool,
    is_symlink: bool,
    hidden: bool,
    name_style: Style,
    palette: &ThemePalette,
) -> Style {
    if is_dir && !is_symlink && hidden {
        return Style::default().fg(Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b));
    }
    name_style
}

/// Style a single character of a Unix attributes string (`drwxrwxrwx`):
/// the `d` matches directories (blue), `l` matches symlinks (cyan), `x` matches
/// executables (green), `w` uses the normal text color, and `r`/`-` use the
/// muted overlay color (same as the inactive tab title). Local files on Windows
/// use a different attribute-flag format, so their attributes keep the plain
/// text color.
fn attributes_cell(
    attrs: &str,
    palette: &ThemePalette,
    text_fg: Color,
    unix_style: bool,
) -> Cell<'static> {
    let default_style = Style::default().fg(text_fg);
    if unix_style {
        let spans: Vec<Span> = attrs
            .chars()
            .map(|c| Span::styled(c.to_string(), attributes_char_style(c, palette, text_fg)))
            .collect();
        Cell::from(Line::from(spans))
    } else {
        Cell::from(attrs.to_string()).style(default_style)
    }
}

/// Style of a single character of a Unix attributes string.
pub(super) fn attributes_char_style(c: char, palette: &ThemePalette, text_fg: Color) -> Style {
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
    let color = match c {
        'd' => rgb(palette.blue),
        'l' => rgb(palette.cyan),
        'x' => rgb(palette.green),
        'r' | '-' => rgb(palette.overlay0),
        _ => text_fg,
    };
    Style::default().fg(color)
}

/// Nerd-font icon for an entry, or an empty string when the filesystem does not
/// provide one.
fn entry_icon(entry: &FileEntry, current_dir: &Path) -> &'static str {
    crate::icons::get_icon(
        &entry.name,
        entry.is_dir,
        entry.is_symlink,
        crate::fs::utils::is_executable(&current_dir.join(&entry.name), entry),
    )
}

/// Render a single entry row
pub(super) fn render_entry_row<'a>(
    entry: &'a FileEntry,
    idx: usize,
    ctx: &'a EntryRowContext<'a>,
    search_highlights: &'a HashMap<usize, Vec<usize>>,
) -> Row<'a> {
    let text_fg = Color::Rgb(ctx.palette.text.r, ctx.palette.text.g, ctx.palette.text.b);
    let name_style = style_for_entry(entry, ctx.current_dir, ctx.palette, ctx.unix_attributes);
    let icon_style = style_for_icon(entry, ctx.palette, name_style, ctx.unix_attributes);

    // Account for ratatui border: subtract 2 from available width
    // Also account for icon width (2 chars: icon + space) if icons are enabled
    let icon_width = if ctx.icons_enabled { 2 } else { 0 };
    let visible_name_width = if ctx.name_col_width > icon_width {
        ctx.name_col_width - icon_width
    } else {
        1
    };

    let name_cell = if let Some(matches) = search_highlights.get(&idx) {
        let yellow = Color::Rgb(
            ctx.palette.yellow.r,
            ctx.palette.yellow.g,
            ctx.palette.yellow.b,
        );
        let highlight_style = Style::default().fg(yellow).add_modifier(Modifier::BOLD);

        let name_chars: Vec<char> = entry.name.chars().collect();
        let name_len = name_chars.len();
        let mut spans = Vec::new();

        // Add icon if enabled
        if ctx.icons_enabled {
            let icon = entry_icon(entry, ctx.current_dir);
            spans.push(Span::styled(format!("{icon} "), icon_style));
        }

        if name_len <= visible_name_width {
            for (i, c) in name_chars.iter().enumerate() {
                if matches.contains(&i) {
                    spans.push(Span::styled(c.to_string(), highlight_style));
                } else {
                    spans.push(Span::raw(c.to_string()));
                }
            }
        } else {
            // Truncate logic
            let ellipsis = "…";
            let ellipsis_len = 1;
            let keep = visible_name_width.saturating_sub(ellipsis_len);
            let left = keep / 2;
            let right = keep - left;

            // Left part
            for (i, c) in name_chars.iter().take(left).enumerate() {
                if matches.contains(&i) {
                    spans.push(Span::styled(c.to_string(), highlight_style));
                } else {
                    spans.push(Span::raw(c.to_string()));
                }
            }
            // Ellipsis
            spans.push(Span::raw(ellipsis));
            // Right part
            let start_right = name_len.saturating_sub(right);
            for (i, c) in name_chars.iter().skip(start_right).enumerate() {
                let original_idx = start_right + i;
                if matches.contains(&original_idx) {
                    spans.push(Span::styled(c.to_string(), highlight_style));
                } else {
                    spans.push(Span::raw(c.to_string()));
                }
            }
        }
        Cell::from(Line::from(spans)).style(name_style)
    } else {
        let truncated_name = truncate_middle_with_ellipsis(&entry.name, visible_name_width);
        if ctx.icons_enabled {
            let icon = entry_icon(entry, ctx.current_dir);
            Cell::from(Line::from(vec![
                Span::styled(format!("{icon} "), icon_style),
                Span::raw(truncated_name),
            ]))
            .style(name_style)
        } else {
            Cell::from(truncated_name).style(name_style)
        }
    };

    // Check if we have a cached size for this directory
    let size_to_display = if entry.is_dir && entry.name != ".." {
        let full_path = ctx.current_dir.join(&entry.name);
        ctx.dir_sizes.get(&full_path).copied()
    } else {
        None
    };

    Row::new(vec![
        name_cell,
        Cell::from(format_size(
            size_to_display.or(entry.size),
            entry.is_dir && size_to_display.is_none(),
            entry.is_symlink,
        ))
        .style(Style::default().fg(text_fg)),
        Cell::from(format_modified(entry.modified)).style(Style::default().fg(text_fg)),
        attributes_cell(
            entry.attributes.as_str(),
            ctx.palette,
            text_fg,
            ctx.unix_attributes,
        ),
    ])
}
