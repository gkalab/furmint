use crate::state::{DirectoryListingRow, FileViewerSearchState, FileViewerState};
use crate::theme::ThemePalette;
use crate::ui::ui_utils::TabScrollbarContext;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

/// A styled run of one line: its resolved foreground plus the run's text.
type Segment<'a> = (Option<Color>, &'a str);

fn normalize_selection(sel: ((usize, usize), (usize, usize))) -> ((usize, usize), (usize, usize)) {
    let ((r1, c1), (r2, c2)) = sel;
    if r1 < r2 || (r1 == r2 && c1 <= c2) {
        sel
    } else {
        ((r2, c2), (r1, c1))
    }
}

pub fn draw_file_viewer(
    f: &mut ratatui::Frame,
    viewer: &mut FileViewerState,
    area: Rect,
    geometry: &crate::layout::ViewerGeometry,
    palette: &ThemePalette,
    borders: bool,
    icons_enabled: bool,
) {
    let title = crate::ui::ui_utils::replace_home_with_tilde(&viewer.path);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(Style::default().fg(if viewer.focused {
            Color::Rgb(palette.border.r, palette.border.g, palette.border.b)
        } else {
            Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b)
        }));

    let block = if borders {
        block.border_type(ratatui::widgets::BorderType::Rounded)
    } else {
        block.border_set(ratatui::symbols::border::EMPTY)
    };

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    if viewer
        .image
        .protocol
        .as_ref()
        .and_then(|p| p.protocol_type())
        .is_some()
    {
        viewer.prepare_image_protocol(geometry);
        let resize = if viewer.is_image_zoomed(geometry) {
            ratatui_image::Resize::Scale(Some(ratatui_image::FilterType::CatmullRom))
        } else {
            ratatui_image::Resize::Fit(Some(ratatui_image::FilterType::CatmullRom))
        };
        if let Some(protocol) = &mut viewer.image.protocol {
            f.render_stateful_widget(
                ratatui_image::StatefulImage::new().resize(resize),
                inner_area,
                protocol,
            );
        }
        return;
    }

    // Wrapped content (e.g. `file` output for a binary) is re-flowed to the
    // current width here, where the width is known.
    viewer.text.rewrap(inner_area.width as usize);

    if viewer.text.content.is_empty() && viewer.text.large_file_indexer.is_none() {
        if viewer.is_loading {
            let loading_style = Style::default().fg(Color::Rgb(
                palette.overlay0.r,
                palette.overlay0.g,
                palette.overlay0.b,
            ));
            f.render_widget(
                Paragraph::new(Line::from(Span::styled("  Loading...", loading_style))),
                inner_area,
            );
        }
        return;
    }

    render_text_content(
        f,
        viewer,
        inner_area,
        geometry,
        palette,
        borders,
        icons_enabled,
    );
}

fn render_text_content(
    f: &mut ratatui::Frame,
    viewer: &mut FileViewerState,
    inner_area: Rect,
    geometry: &crate::layout::ViewerGeometry,
    palette: &ThemePalette,
    borders: bool,
    icons_enabled: bool,
) {
    let visible_lines = FileViewerState::visible_lines(geometry);
    let (max_lines, is_large_file) = if let Some(indexer) = &viewer.text.large_file_indexer {
        (indexer.total_lines(), true)
    } else {
        (viewer.text.content.len(), false)
    };

    let max_scroll = max_lines.saturating_sub(visible_lines);
    let start_line = viewer.text.scroll_offset.min(max_scroll);
    let end_line = (start_line + visible_lines).min(max_lines);

    // Ask for anything the highlight worker has not returned yet. Lines that are
    // still missing render unstyled; the draw pass never waits for a compile.
    viewer.text.request_highlights(start_line, end_line);

    let default_fg = viewer
        .text
        .theme
        .as_ref()
        .and_then(|t| crate::state::parse_hex_color(t.fg()));

    let normalized_selection = viewer.text.selection.map(normalize_selection);

    let mut lines = Vec::new();
    for i in start_line..end_line {
        if let Some(line) = render_viewer_line(&ViewerLineContext {
            line_idx: i,
            viewer,
            is_large_file,
            segments: viewer.text.line_segments(i),
            default_fg,
            normalized_selection,
            max_width: inner_area.width as usize,
            palette,
            icons_enabled,
        }) {
            lines.push(line);
        }
    }

    f.render_widget(Paragraph::new(lines), inner_area);

    let outer = geometry.area;
    let scroll_area = Rect {
        x: outer.x + outer.width - 1,
        y: outer.y + 1,
        width: 1,
        height: outer.height.saturating_sub(2),
    };

    crate::ui::ui_utils::draw_tab_scrollbar(
        f,
        scroll_area,
        max_lines,
        visible_lines,
        viewer.text.scroll_offset,
        &TabScrollbarContext {
            palette,
            borders,
            is_root: false,
            active: viewer.focused,
        },
        true,
    );
}

pub fn draw_viewer_search_popup(
    f: &mut ratatui::Frame,
    state: &FileViewerSearchState,
    palette: &ThemePalette,
) {
    if !state.is_visible {
        return;
    }

    crate::ui::ui_utils::draw_input_popup(
        f,
        &crate::ui::ui_utils::InputPopupOptions {
            title: Some("Search"),
            input: &state.query,
            error: state.error.as_deref(),
            placeholder: "Enter regex...",
            width: 60,
        },
        palette,
    );
}

struct ViewerLineContext<'a> {
    line_idx: usize,
    viewer: &'a FileViewerState,
    is_large_file: bool,
    /// Styled runs for this line, or `None` while the worker is still busy. A
    /// line without runs renders as plain text rather than waiting.
    segments: Option<&'a [(Option<Color>, String)]>,
    default_fg: Option<Color>,
    normalized_selection: Option<((usize, usize), (usize, usize))>,
    max_width: usize,
    palette: &'a ThemePalette,
    icons_enabled: bool,
}

fn format_archive_size(size: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    if size >= 1_073_741_824 {
        let val = size as f64 / 1_073_741_824.0;
        let s = format!("{val:.1}");
        format!("{}G", s.trim_end_matches(".0"))
    } else if size >= 1_048_576 {
        let val = size as f64 / 1_048_576.0;
        let s = format!("{val:.1}");
        format!("{}M", s.trim_end_matches(".0"))
    } else if size >= 1_024 {
        let val = size as f64 / 1_024.0;
        let s = format!("{val:.1}");
        format!("{}K", s.trim_end_matches(".0"))
    } else {
        format!("{size}B")
    }
}

fn render_archive_line(
    ctx: &ViewerLineContext,
    row: &crate::fs::archive::preview::ArchiveTreeRow,
) -> Line<'static> {
    let overlay_color = Color::Rgb(
        ctx.palette.overlay0.r,
        ctx.palette.overlay0.g,
        ctx.palette.overlay0.b,
    );
    let subtext_color = Color::Rgb(
        ctx.palette.subtext.r,
        ctx.palette.subtext.g,
        ctx.palette.subtext.b,
    );

    let icon = if ctx.icons_enabled {
        crate::icons::get_icon(&row.name, row.is_dir, false, false)
    } else {
        ""
    };
    let size_str = if row.is_dir {
        String::new()
    } else {
        row.size.map(format_archive_size).unwrap_or_default()
    };

    let search_fg = Color::Rgb(ctx.palette.base.r, ctx.palette.base.g, ctx.palette.base.b);
    let highlight_bg = Color::Rgb(
        ctx.palette.yellow.r,
        ctx.palette.yellow.g,
        ctx.palette.yellow.b,
    );

    let prefix_chars = row.prefix.chars().count();
    let icon_chars = icon.chars().count();
    let icon_offset = if icon.is_empty() { 0 } else { icon_chars + 1 };
    let map_content_to_rendered = |c: usize| {
        if c < prefix_chars { c } else { c + icon_offset }
    };

    let search_ranges: Vec<(usize, usize)> = ctx
        .viewer
        .text
        .current_search_match
        .filter(|(line, _, _)| *line == ctx.line_idx)
        .map(|(_, s, e)| (map_content_to_rendered(s), map_content_to_rendered(e)))
        .into_iter()
        .collect();

    let highlights = LineHighlights {
        selection: None,
        search_ranges: &search_ranges,
        search_fg,
        search_bg: highlight_bg,
    };

    let mut spans = Vec::new();
    let mut seg_start = 0;
    let prefix_style = Style::default().fg(overlay_color);
    let hidden =
        crate::fs::utils::has_hidden_name(&row.name, crate::fs::utils::has_unix_attributes(false));
    let entry_style =
        crate::ui::panel::entry_foreground(row.is_dir, false, hidden, false, ctx.palette);
    // Hidden directories keep the standard directory blue on their icon, so the
    // row still reads as a directory while the muted name signals "hidden".
    let icon_style = crate::ui::panel::entry_icon_foreground(
        row.is_dir,
        false,
        hidden,
        entry_style,
        ctx.palette,
    );

    spans.extend(highlights.render(&row.prefix, seg_start, prefix_style));
    seg_start += prefix_chars;
    if !icon.is_empty() {
        spans.extend(highlights.render(&format!("{icon} "), seg_start, icon_style));
        seg_start += icon_chars + 1;
    }
    spans.extend(highlights.render(&row.name, seg_start, entry_style));

    let left_width = unicode_width::UnicodeWidthStr::width(row.prefix.as_str())
        + unicode_width::UnicodeWidthStr::width(icon)
        + usize::from(!icon.is_empty())
        + unicode_width::UnicodeWidthStr::width(row.name.as_str());

    if !size_str.is_empty() && ctx.max_width > left_width + size_str.len() {
        let pad_len = ctx.max_width - left_width - size_str.len();
        spans.push(Span::raw(" ".repeat(pad_len)));
        spans.push(Span::styled(size_str, Style::default().fg(subtext_color)));
    }

    Line::from(spans)
}

/// Renders one line of the viewer's directory preview.
///
/// Entries are colored with the same rules as a tab listing (see
/// [`crate::ui::panel::entry_foreground`]); the header line shows the selected
/// directory's full path and, when it is a symlink, its target.
fn render_directory_line(ctx: &ViewerLineContext, row: &DirectoryListingRow) -> Line<'static> {
    let name = &row.entry.name;
    let is_dir = row.entry.is_dir;
    let is_symlink = row.entry.is_symlink;
    // The header is the directory's own path, not an entry, so it keeps the
    // normal text color instead of the directory blue a listing entry would use.
    let name_style = if row.is_header {
        Style::default().fg(Color::Rgb(
            ctx.palette.text.r,
            ctx.palette.text.g,
            ctx.palette.text.b,
        ))
    } else {
        crate::ui::panel::entry_foreground(
            is_dir,
            is_symlink,
            row.hidden,
            row.executable,
            ctx.palette,
        )
    };
    let icon_style = crate::ui::panel::entry_icon_foreground(
        is_dir,
        is_symlink,
        row.hidden,
        name_style,
        ctx.palette,
    );
    let target_style = Style::default().fg(Color::Rgb(
        ctx.palette.cyan.r,
        ctx.palette.cyan.g,
        ctx.palette.cyan.b,
    ));

    // Only listing entries get an icon; the header is the directory's path.
    let icon = if ctx.icons_enabled && !row.is_header {
        crate::icons::get_icon(name, is_dir, is_symlink, row.executable)
    } else {
        ""
    };
    let icon_chars = icon.chars().count();
    // The rendered prefix is the authoritative offset the mouse hit-testing also
    // uses, so both stay in sync.
    let icon_offset = ctx
        .viewer
        .rendered_row_prefix(ctx.line_idx, ctx.icons_enabled);

    let search_ranges: Vec<(usize, usize)> = ctx
        .viewer
        .text
        .current_search_match
        .filter(|(line, _, _)| *line == ctx.line_idx)
        .map(|(_, s, e)| (s.saturating_add(icon_offset), e.saturating_add(icon_offset)))
        .into_iter()
        .collect();

    // Selection ranges are stored in `content` coordinates; shift them past the
    // icon to match the rendered line.
    let selection_range: Option<(usize, usize)> = ctx
        .normalized_selection
        .and_then(|((r1, c1), (r2, c2))| selection_for_line(ctx.line_idx, r1, c1, r2, c2))
        .map(|(s, e)| (s.saturating_add(icon_offset), e.saturating_add(icon_offset)));

    let selection_bg = Color::Rgb(
        ctx.palette.surface0.r,
        ctx.palette.surface0.g,
        ctx.palette.surface0.b,
    );
    let highlights = LineHighlights {
        selection: selection_range.map(|range| (range, selection_bg)),
        search_ranges: &search_ranges,
        search_fg: Color::Rgb(ctx.palette.base.r, ctx.palette.base.g, ctx.palette.base.b),
        search_bg: Color::Rgb(
            ctx.palette.yellow.r,
            ctx.palette.yellow.g,
            ctx.palette.yellow.b,
        ),
    };

    let mut spans = Vec::new();
    let mut seg_start = 0;
    if !icon.is_empty() {
        spans.extend(highlights.render(&format!("{icon} "), seg_start, icon_style));
        seg_start += icon_chars + 1;
    }
    spans.extend(highlights.render(name, seg_start, name_style));
    seg_start += name.chars().count();
    if let Some(target) = &row.link_target {
        spans.extend(highlights.render(&format!(" -> {target}"), seg_start, target_style));
    }

    Line::from(spans)
}

/// The selected character range (in `content` coordinates) on `line_idx` for a
/// normalized selection, or `None` when the selection does not touch the line.
fn selection_for_line(
    line_idx: usize,
    r1: usize,
    c1: usize,
    r2: usize,
    c2: usize,
) -> Option<(usize, usize)> {
    if line_idx < r1 || line_idx > r2 {
        None
    } else if line_idx > r1 && line_idx < r2 {
        Some((0, usize::MAX))
    } else if r1 == r2 {
        Some((c1, c2))
    } else if line_idx == r1 {
        Some((c1, usize::MAX))
    } else {
        Some((0, c2))
    }
}

/// Selection and search ranges (in rendered columns) plus their colors, layered
/// onto the styled runs of a line that is assembled segment by segment.
///
/// Shared by the archive tree and the directory preview, which both color a line
/// run by run and need the viewer's search highlight (and, for the directory
/// preview, the text selection) painted on top.
struct LineHighlights<'a> {
    selection: Option<((usize, usize), Color)>,
    search_ranges: &'a [(usize, usize)],
    search_fg: Color,
    search_bg: Color,
}

impl LineHighlights<'_> {
    fn render(&self, text: &str, seg_start: usize, style: Style) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (pos, ch) in (seg_start..).zip(text.chars()) {
            let mut char_style = style;
            if let Some(((s, e), bg)) = self.selection
                && pos >= s
                && pos < e
            {
                char_style = char_style.bg(bg);
            }
            if self
                .search_ranges
                .iter()
                .any(|(s, e)| pos >= *s && pos < *e)
            {
                char_style = char_style.fg(self.search_fg).bg(self.search_bg);
            }
            if let Some(last) = spans.last_mut()
                && last.style == char_style
            {
                let mut new_content = last.content.to_string();
                new_content.push(ch);
                last.content = new_content.into();
            } else {
                spans.push(Span::styled(ch.to_string(), char_style));
            }
        }
        spans
    }
}

fn render_viewer_line(ctx: &ViewerLineContext) -> Option<Line<'static>> {
    if let Some(rows) = &ctx.viewer.text.directory_rows
        && let Some(row) = rows.get(ctx.line_idx)
    {
        return Some(render_directory_line(ctx, row));
    }

    if let Some(rows) = &ctx.viewer.text.archive_rows
        && let Some(row) = rows.get(ctx.line_idx)
    {
        return Some(render_archive_line(ctx, row));
    }

    let selection_range = ctx
        .normalized_selection
        .and_then(|((r1, c1), (r2, c2))| selection_for_line(ctx.line_idx, r1, c1, r2, c2));

    let line_content = if ctx.is_large_file {
        ctx.viewer
            .text
            .large_file_indexer
            .as_ref()
            .and_then(|indexer| {
                ctx.viewer
                    .text
                    .large_file_reader
                    .as_ref()
                    .and_then(|reader| {
                        indexer
                            .get_line_with_reader(ctx.line_idx, reader)
                            .map(|(s, e)| reader.get_chunk(s, e))
                    })
            })
    } else {
        ctx.viewer.text.content.get(ctx.line_idx).cloned()
    };

    let line_content = line_content?;

    let search_ranges: Vec<(usize, usize)> = ctx
        .viewer
        .text
        .current_search_match
        .filter(|(line, _, _)| *line == ctx.line_idx)
        .map(|(_, s, e)| (s, e))
        .into_iter()
        .collect();

    let search_fg = Some(Color::Rgb(
        ctx.palette.base.r,
        ctx.palette.base.g,
        ctx.palette.base.b,
    ));

    // Runs come from the highlight worker. Large files and lines it has not
    // returned yet fall back to a single unstyled run over the whole line.
    let segments: Vec<Segment<'_>> = match ctx.segments {
        Some(segments) if !segments.is_empty() => segments
            .iter()
            .map(|(color, text)| (*color, text.as_str()))
            .collect(),
        _ => vec![(None, line_content.as_str())],
    };

    let spans = generate_line_spans(&LineSpansContext {
        segments: &segments,
        h_offset: ctx.viewer.text.horizontal_scroll_offset,
        max_width: ctx.max_width,
        default_fg: ctx.default_fg,
        selection: selection_range,
        selection_bg: Some(Color::Rgb(
            ctx.palette.surface0.r,
            ctx.palette.surface0.g,
            ctx.palette.surface0.b,
        )),
        search_ranges: &search_ranges,
        search_bg: Some(Color::Rgb(
            ctx.palette.yellow.r,
            ctx.palette.yellow.g,
            ctx.palette.yellow.b,
        )),
        search_fg,
    });

    Some(Line::from(spans))
}

/// Generates spans for a single line, handling horizontal scrolling and width constraints
/// taking into account tab widths and wide characters.
#[must_use]
pub struct LineSpansContext<'a> {
    /// The line's styled runs, as resolved foreground plus text.
    pub segments: &'a [Segment<'a>],
    pub h_offset: usize,
    pub max_width: usize,
    pub default_fg: Option<Color>,
    pub selection: Option<(usize, usize)>,
    pub selection_bg: Option<Color>,
    pub search_ranges: &'a [(usize, usize)],
    pub search_bg: Option<Color>,
    pub search_fg: Option<Color>,
}

impl LineSpansContext<'_> {
    fn calculate_text_display_width(text: &str, start_pos: usize) -> usize {
        let mut width = 0;
        let mut pos = start_pos;
        for ch in text.chars() {
            let w = if ch == '\t' {
                4 - (pos % 4)
            } else {
                unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
            };
            width += w;
            pos += w;
        }
        width
    }

    /// Decides what a single character contributes to the rendered line.
    ///
    /// Returns `Some((true, width))` for a tab that has to be emitted as that
    /// many spaces, `Some((false, width))` for the character itself, and `None`
    /// for a character that falls outside the visible window or would overflow
    /// it.
    fn visible_char(
        &self,
        ch: char,
        current_pos: usize,
        visible_width: usize,
    ) -> Option<(bool, usize)> {
        let ch_width = if ch == '\t' {
            4 - (current_pos % 4)
        } else {
            unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
        };

        let ch_end_pos = current_pos + ch_width;
        if ch_end_pos <= self.h_offset || current_pos >= self.h_offset + self.max_width {
            return None;
        }

        if current_pos >= self.h_offset {
            if visible_width + ch_width <= self.max_width {
                return Some((ch == '\t', ch_width));
            }
        } else if ch == '\t' {
            let visible_tab_width = ch_end_pos - self.h_offset;
            if visible_width + visible_tab_width <= self.max_width {
                return Some((true, visible_tab_width));
            }
        }

        None
    }

    fn get_char_style(&self, char_idx: usize, base_fg: Option<Color>) -> ratatui::style::Style {
        let mut style = if let Some(color) = base_fg {
            ratatui::style::Style::default().fg(color)
        } else {
            ratatui::style::Style::default()
        };

        if let Some((sel_start, sel_end)) = self.selection
            && char_idx >= sel_start
            && char_idx < sel_end
        {
            style = style.bg(self.selection_bg.unwrap_or(Color::Rgb(60, 60, 60)));
        }

        for (s, e) in self.search_ranges {
            if char_idx >= *s && char_idx < *e {
                if let Some(bg) = self.search_bg {
                    style = style.bg(bg);
                }
                if let Some(fg) = self.search_fg {
                    style = style.fg(fg);
                }
                break;
            }
        }
        style
    }
}

#[must_use]
pub fn generate_line_spans(ctx: &LineSpansContext<'_>) -> Vec<Span<'static>> {
    let mut display_pos = 0;
    let mut visible_width = 0;
    let mut char_idx = 0;

    let mut spans: Vec<Span<'static>> = Vec::new();
    // Text for the run currently being built. Appending into one buffer keeps
    // this linear: the previous version reallocated the whole run per character.
    let mut run = String::new();
    let mut run_style: Option<Style> = None;

    for (color, text) in ctx.segments {
        if visible_width >= ctx.max_width {
            break;
        }

        let text_width = LineSpansContext::calculate_text_display_width(text, display_pos);
        let end_pos = display_pos + text_width;

        if end_pos > ctx.h_offset {
            let base_fg = (*color).or(ctx.default_fg);
            let mut current_pos = display_pos;

            for ch in text.chars() {
                if let Some((as_spaces, width)) = ctx.visible_char(ch, current_pos, visible_width) {
                    let style = ctx.get_char_style(char_idx, base_fg);
                    if run_style != Some(style) {
                        if !run.is_empty()
                            && let Some(previous) = run_style
                        {
                            spans.push(Span::styled(std::mem::take(&mut run), previous));
                        }
                        run_style = Some(style);
                    }
                    if as_spaces {
                        run.extend(std::iter::repeat_n(' ', width));
                    } else {
                        run.push(ch);
                    }
                    visible_width += width;
                }

                let ch_w = if ch == '\t' {
                    4 - (current_pos % 4)
                } else {
                    unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
                };
                current_pos += ch_w;
                char_idx += 1;
                if visible_width >= ctx.max_width {
                    break;
                }
            }
        } else {
            char_idx += text.chars().count();
        }
        display_pos = end_pos;
    }

    if !run.is_empty()
        && let Some(style) = run_style
    {
        spans.push(Span::styled(run, style));
    }
    spans
}

#[cfg(test)]
mod tests;
