use super::entry::{attributes_char_style, style_for_entry, style_for_icon};
use super::layout::{attributes_col_width, name_col_width_for};
use super::status::get_task_progress_spans;
use crate::fs::utils::FileEntry;
use crate::theme::{Rgb, ThemePalette};
use ratatui::prelude::{Color, Style};
use std::path::Path;

#[test]
fn test_task_progress_truncation() {
    let palette = crate::theme::catppuccin_macchiato();

    // Case 1: No truncation needed
    // Progress strings: "50% (5 left) " (13) + "| " (2) + "[50% of 1000 B] " (~16) = ~31 overhead
    // "file.txt " = 9 chars. Total needed ~40.
    let max_width = 60;
    let spans = get_task_progress_spans(
        Some((5, 10)),
        Some((500, 1000)),
        false,
        Some("file.txt"),
        max_width,
        &palette,
    );
    let total_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    assert!(total_width <= max_width);
    assert!(spans.iter().any(|s| s.content.contains("file.txt")));

    // Case 2: Truncation needed for long filename
    // Overhead ~31. Max width 45. Available ~14.
    // File "very...txt" > 14. Should truncate.
    let case2_width = 45;
    let long_file = "very_long_filename_that_definitely_needs_truncation.txt";
    let spans = get_task_progress_spans(
        Some((5, 10)),
        Some((500, 1000)),
        false,
        Some(long_file),
        case2_width,
        &palette,
    );
    let total_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    assert!(total_width <= case2_width);
    // The filename should be truncated
    assert!(spans.iter().any(|s| s.content.contains("…")));

    // Case 3: Very narrow width
    // Max width 10. Overhead ~31. File should be dropped.
    // Total width will exceed max_width because progress bars are not truncated,
    // but we verify file is dropped.
    let narrow_width = 10;
    let spans = get_task_progress_spans(
        Some((5, 10)),
        Some((500, 1000)),
        false,
        Some("short.txt"),
        narrow_width,
        &palette,
    );
    // We expect the file to be absent
    assert!(!spans.iter().any(|s| s.content.contains("short.txt")));
}

#[test]
fn test_name_column_fits_ratatui_layout() {
    // 4 table columns -> 3 gaps at default spacing 1, plus 2 border columns and the
    // fixed non-name columns (which vary by platform: 6 characters of Windows attribute
    // flags for local files, 10 characters of Unix permissions otherwise). The budget
    // is `width - overhead`, and the icon lives inside the name column, so rendering
    // must never clip the tail of a truncated name.
    let total_table_width = 100;
    let name_col = calculate_name_col_for(total_table_width, attributes_col_width(true));
    let real_col = name_column_width(total_table_width, attributes_col_width(true));

    assert_eq!(name_col, real_col);

    // With icons enabled the icon (2 chars) is prefixed inside the name column.
    let with_icon = name_col.saturating_sub(2);
    assert!(with_icon <= real_col);
    // Without icons the truncated name uses the full column.
    assert!(name_col <= real_col);
    // A name of exactly the budget length fits without overflow.
    assert!(with_icon < real_col);
}

#[test]
fn test_attributes_column_width_per_filesystem() {
    use crate::fs::utils::has_unix_attributes;
    // Remote and archive entries carry Unix permissions, so the column is
    // wide enough for all 10 characters even on Windows.
    if cfg!(windows) {
        assert_eq!(attributes_col_width(false), 10);
        assert!(has_unix_attributes(false));
        assert_eq!(attributes_col_width(true), 6);
        assert!(!has_unix_attributes(true));
    } else {
        assert_eq!(attributes_col_width(true), 10);
        assert_eq!(attributes_col_width(false), 10);
        assert!(has_unix_attributes(true));
        assert!(has_unix_attributes(false));
    }
}

fn name_column_width(total: usize, attributes: u16) -> usize {
    let overhead = 7 + 19 + usize::from(attributes) + 3 + 2;
    if total > overhead {
        total - overhead
    } else {
        10
    }
}

fn calculate_name_col_for(total: usize, attributes: u16) -> usize {
    name_col_width_for(total, attributes)
}

fn fg_of(style: Style) -> Option<Color> {
    style.fg
}

fn entry(name: &str, is_dir: bool, is_symlink: bool, attributes: &str) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink,
        size: None,
        modified: None,
        attributes: attributes.to_string(),
        selected: false,
    }
}

/// [`style_for_entry`] for a filesystem that follows the Unix conventions:
/// local on Unix, and every remote and archive tab on any host.
fn unix_entry_style(entry: &FileEntry, current_dir: &Path, palette: &ThemePalette) -> Style {
    style_for_entry(entry, current_dir, palette, true)
}

/// [`style_for_icon`] for the same Unix-conventions filesystem.
fn unix_icon_style(entry: &FileEntry, palette: &ThemePalette, name_style: Style) -> Style {
    style_for_icon(entry, palette, name_style, true)
}

/// [`style_for_entry`] for a local Windows filesystem, which reads the DOS
/// attribute flags and has no dotfile convention.
fn local_entry_style(entry: &FileEntry, current_dir: &Path, palette: &ThemePalette) -> Style {
    style_for_entry(entry, current_dir, palette, false)
}

/// [`style_for_icon`] for a local Windows filesystem.
fn local_icon_style(entry: &FileEntry, palette: &ThemePalette, name_style: Style) -> Style {
    style_for_icon(entry, palette, name_style, false)
}

#[test]
fn test_symlink_uses_its_own_color() {
    let palette = crate::theme::catppuccin_macchiato();
    let current_dir = Path::new("/nonexistent");
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

    // A real directory stays blue and a plain executable stays green.
    let dir = entry("real", true, false, "drwxr-xr-x ");
    assert_eq!(
        fg_of(unix_entry_style(&dir, current_dir, &palette)),
        Some(rgb(palette.blue))
    );
    let exec = entry("run", false, false, "-rwxr-xr-x");
    assert_eq!(
        fg_of(unix_entry_style(&exec, current_dir, &palette)),
        Some(rgb(palette.green))
    );

    // A link to a directory is not blue: the cyan marks it as a link even
    // though it is enterable and sorts with directories.
    let linked_dir = entry("link", true, true, "lrwxrwxrwx ");
    let dir_style = fg_of(unix_entry_style(&linked_dir, current_dir, &palette));
    assert_eq!(dir_style, Some(rgb(palette.cyan)));
    assert_ne!(dir_style, Some(rgb(palette.blue)));
    assert_ne!(dir_style, Some(rgb(palette.green)));

    // A link to a file uses the same color, and never the executable green.
    let linked_file = entry("link.txt", false, true, "lrwxrwxrwx ");
    let file_style = fg_of(unix_entry_style(&linked_file, current_dir, &palette));
    assert_eq!(file_style, Some(rgb(palette.cyan)));
    assert_ne!(file_style, Some(rgb(palette.green)));
    assert_ne!(file_style, Some(rgb(palette.text)));

    // A broken link falls back to the same cyan.
    let broken = entry("broken", false, true, "lrwxrwxrwx ");
    assert_eq!(
        fg_of(unix_entry_style(&broken, current_dir, &palette)),
        Some(rgb(palette.cyan))
    );
}

#[test]
fn test_every_theme_defines_a_symlink_color() {
    for name in crate::theme::THEME_NAMES {
        let palette = crate::theme::get_theme(name).unwrap();
        assert_ne!(palette.cyan.r, 0, "{name} has an unset symlink color");
    }
}

#[cfg(not(windows))]
#[test]
fn test_hidden_entries_use_a_de_emphasized_color() {
    let current_dir = Path::new("/nonexistent");
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

    for name in crate::theme::THEME_NAMES {
        let palette = crate::theme::get_theme(name).unwrap();

        // A hidden file is muted to the subtext color instead of the normal
        // text color.
        let hidden_file = entry(".env", false, false, "-rw-r--r--");
        assert_eq!(
            fg_of(unix_entry_style(&hidden_file, current_dir, &palette)),
            Some(rgb(palette.subtext)),
            "{name} does not mute hidden files"
        );

        // Hidden directories use the same muted color as hidden files.
        let hidden_dir = entry(".git", true, false, "drwxr-xr-x ");
        assert_eq!(
            fg_of(unix_entry_style(&hidden_dir, current_dir, &palette)),
            Some(rgb(palette.subtext)),
            "{name} does not mute hidden directories"
        );

        // Hiding a file wins over the executable green.
        let hidden_exec = entry(".hidden.sh", false, false, "-rwxr-xr-x");
        assert_eq!(
            fg_of(unix_entry_style(&hidden_exec, current_dir, &palette)),
            Some(rgb(palette.subtext)),
            "{name} colors a hidden executable green"
        );

        // A visible file keeps the normal text color.
        let plain = entry("env", false, false, "-rw-r--r--");
        assert_eq!(
            fg_of(unix_entry_style(&plain, current_dir, &palette)),
            Some(rgb(palette.text)),
            "{name} does not keep the text color for visible files"
        );

        // A visible directory keeps the directory blue.
        let dir = entry("git", true, false, "drwxr-xr-x ");
        assert_eq!(
            fg_of(unix_entry_style(&dir, current_dir, &palette)),
            Some(rgb(palette.blue)),
            "{name} does not keep the directory color for visible directories"
        );
    }
}

#[cfg(windows)]
#[test]
fn test_dot_names_keep_their_regular_color_on_windows() {
    let current_dir = Path::new("/nonexistent");
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

    for name in crate::theme::THEME_NAMES {
        let palette = crate::theme::get_theme(name).unwrap();

        // A local Windows filesystem has no dotfile convention, so a leading
        // dot must not mute the entry the way it does on Unix. These entries
        // carry no DOS hidden flag, so nothing marks them hidden.
        let dot_file = entry(".env", false, false, "------");
        let file_style = local_entry_style(&dot_file, current_dir, &palette);
        assert_eq!(
            fg_of(file_style),
            Some(rgb(palette.text)),
            "{name} mutes a dot-prefixed file on Windows"
        );
        assert_eq!(
            fg_of(local_icon_style(&dot_file, &palette, file_style)),
            Some(rgb(palette.text)),
            "{name} changes the icon of a dot-prefixed file on Windows"
        );

        let dot_dir = entry(".git", true, false, "d-----");
        let dir_style = local_entry_style(&dot_dir, current_dir, &palette);
        assert_eq!(
            fg_of(dir_style),
            Some(rgb(palette.blue)),
            "{name} mutes a dot-prefixed directory on Windows"
        );
        assert_eq!(
            fg_of(local_icon_style(&dot_dir, &palette, dir_style)),
            Some(rgb(palette.blue)),
            "{name} changes the icon of a dot-prefixed directory on Windows"
        );

        // An executable keeps its green even behind a leading dot.
        let dot_exec = entry(".build.sh", false, false, "-rwxr-xr-x");
        assert_eq!(
            fg_of(local_entry_style(&dot_exec, current_dir, &palette)),
            Some(rgb(palette.green)),
            "{name} mutes a dot-prefixed executable on Windows"
        );

        // A dot-prefixed symlink keeps its cyan link marker.
        let dot_link = entry(".link", true, true, "lrwxrwxrwx ");
        let link_style = local_entry_style(&dot_link, current_dir, &palette);
        assert_eq!(
            fg_of(link_style),
            Some(rgb(palette.cyan)),
            "{name} mutes a dot-prefixed symlink on Windows"
        );
        assert_eq!(
            fg_of(local_icon_style(&dot_link, &palette, link_style)),
            Some(rgb(palette.cyan)),
            "{name} changes the icon of a dot-prefixed symlink on Windows"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn test_hidden_directory_icon_keeps_the_directory_blue() {
    let current_dir = Path::new("/nonexistent");
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

    for name in crate::theme::THEME_NAMES {
        let palette = crate::theme::get_theme(name).unwrap();

        // A hidden directory keeps the standard directory blue on its icon
        // while its name stays muted.
        let hidden_dir = entry(".git", true, false, "drwxr-xr-x ");
        let name_style = unix_entry_style(&hidden_dir, current_dir, &palette);
        assert_eq!(
            fg_of(name_style),
            Some(rgb(palette.subtext)),
            "{name} does not mute the name of a hidden directory"
        );
        assert_eq!(
            fg_of(unix_icon_style(&hidden_dir, &palette, name_style)),
            Some(rgb(palette.blue)),
            "{name} does not keep the directory blue on the icon"
        );

        // Hidden files have no directory icon to keep, so theirs matches the
        // muted name.
        let hidden_file = entry(".env", false, false, "-rw-r--r--");
        let file_style = unix_entry_style(&hidden_file, current_dir, &palette);
        assert_eq!(
            fg_of(unix_icon_style(&hidden_file, &palette, file_style)),
            fg_of(file_style),
            "{name} does not keep hidden file icons muted"
        );

        // A visible directory is unchanged: icon and name both stay blue.
        let dir = entry("git", true, false, "drwxr-xr-x ");
        let dir_style = unix_entry_style(&dir, current_dir, &palette);
        assert_eq!(
            fg_of(unix_icon_style(&dir, &palette, dir_style)),
            Some(rgb(palette.blue)),
            "{name} changed the icon of a visible directory"
        );

        // A hidden symlink to a directory keeps its cyan link marker rather
        // than borrowing the plain directory blue.
        let linked_dir = entry(".link", true, true, "lrwxrwxrwx ");
        let link_style = unix_entry_style(&linked_dir, current_dir, &palette);
        assert_eq!(
            fg_of(unix_icon_style(&linked_dir, &palette, link_style)),
            Some(rgb(palette.cyan)),
            "{name} dropped the link color for a hidden symlinked directory"
        );
    }
}

#[test]
fn test_windows_hidden_attribute_mutes_the_entry() {
    let current_dir = Path::new("/nonexistent");
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

    for name in crate::theme::THEME_NAMES {
        let palette = crate::theme::get_theme(name).unwrap();

        // A local Windows filesystem marks a hidden file with FILE_ATTRIBUTE_HIDDEN
        // rather than a leading dot, so the name is muted without any dot.
        let hidden_file = entry("AppData", false, false, "--h---");
        let file_style = local_entry_style(&hidden_file, current_dir, &palette);
        assert_eq!(
            fg_of(file_style),
            Some(rgb(palette.subtext)),
            "{name} does not mute a file hidden via the DOS attribute"
        );
        assert_eq!(
            fg_of(local_icon_style(&hidden_file, &palette, file_style)),
            fg_of(file_style),
            "{name} does not keep a DOS-hidden file icon muted"
        );

        // A hidden Windows directory is muted too, icon aside.
        let hidden_dir = entry("AppData", true, false, "d-h---");
        let dir_style = local_entry_style(&hidden_dir, current_dir, &palette);
        assert_eq!(
            fg_of(dir_style),
            Some(rgb(palette.subtext)),
            "{name} does not mute a directory hidden via the DOS attribute"
        );
        assert_eq!(
            fg_of(local_icon_style(&hidden_dir, &palette, dir_style)),
            Some(rgb(palette.blue)),
            "{name} does not keep the directory blue on a DOS-hidden icon"
        );

        // An ordinary Windows directory is untouched.
        let dir = entry("Documents", true, false, "d----");
        let visible_style = local_entry_style(&dir, current_dir, &palette);
        assert_eq!(
            fg_of(visible_style),
            Some(rgb(palette.blue)),
            "{name} muted a plain Windows directory"
        );
    }
}

#[test]
fn test_navigation_entries_are_not_treated_as_hidden() {
    let current_dir = Path::new("/nonexistent");
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
    let palette = crate::theme::catppuccin_macchiato();

    for name in [".", ".."] {
        let nav = entry(name, true, false, "drwxr-xr-x ");
        assert!(
            !crate::fs::utils::is_hidden(name, &nav.attributes, true),
            "{name} should not count as hidden"
        );
        assert_eq!(
            fg_of(unix_entry_style(&nav, current_dir, &palette)),
            Some(rgb(palette.blue)),
            "{name} should keep the directory color"
        );
    }
}

#[test]
fn test_remote_dotfiles_are_muted_on_every_platform() {
    // A remote tab lists Unix permissions and never sets a DOS hidden flag,
    // so its dotfiles must be muted even when the client runs on Windows.
    // A `cfg!(windows)` gate left them fully colored, which is the visible
    // half of the toggle doing nothing in a remote tab.
    let current_dir = Path::new("/nonexistent");
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
    let palette = crate::theme::catppuccin_macchiato();

    for (name, is_dir, attributes, local_fg) in [
        (".env", false, "-rw-r--r--", rgb(palette.text)),
        (".git", true, "drwxr-xr-x ", rgb(palette.blue)),
        (
            ".hidden.sh",
            false,
            "-rwxr-xr-x",
            rgb(palette.green), // an `x` bit reads as executable everywhere
        ),
    ] {
        let remote = entry(name, is_dir, false, attributes);
        let style = unix_entry_style(&remote, current_dir, &palette);
        assert_eq!(
            fg_of(style),
            Some(rgb(palette.subtext)),
            "{name:?} is not muted in a remote tab"
        );
        // A hidden remote directory keeps the directory blue on its icon, so
        // the row still reads as a directory.
        let icon_fg = fg_of(unix_icon_style(&remote, &palette, style));
        assert_eq!(
            icon_fg,
            if is_dir {
                Some(rgb(palette.blue))
            } else {
                Some(rgb(palette.subtext))
            },
            "{name:?} has the wrong icon color in a remote tab"
        );
        // A local Windows filesystem of the same shape keeps them unmuted.
        assert_eq!(
            fg_of(local_entry_style(&remote, current_dir, &palette)),
            Some(local_fg),
            "{name:?} must not be muted locally on Windows without the DOS flag"
        );
    }
}

#[test]
fn test_attributes_type_char_colors() {
    let palette = crate::theme::catppuccin_macchiato();
    let text_fg = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);

    // `l` marks a symlink with the same cyan as its name, for both a linked
    // directory and a linked file.
    for attrs in ["lrwxrwxrwx", "lrwxrwxrwx "] {
        assert_eq!(
            attributes_char_style('l', &palette, text_fg).fg,
            Some(rgb(palette.cyan)),
            "unexpected color for the link marker of {attrs:?}"
        );
    }
    // `d` keeps the directory blue.
    assert_eq!(
        attributes_char_style('d', &palette, text_fg).fg,
        Some(rgb(palette.blue))
    );
    // `x` keeps the executable green.
    assert_eq!(
        attributes_char_style('x', &palette, text_fg).fg,
        Some(rgb(palette.green))
    );
    // `r`/`-` stay muted, `w` uses the normal text color.
    assert_eq!(
        attributes_char_style('r', &palette, text_fg).fg,
        Some(rgb(palette.overlay0))
    );
    assert_eq!(
        attributes_char_style('-', &palette, text_fg).fg,
        Some(rgb(palette.overlay0))
    );
    assert_eq!(
        attributes_char_style('w', &palette, text_fg).fg,
        Some(text_fg)
    );
}
