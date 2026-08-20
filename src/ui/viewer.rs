use crate::app::{FileViewerSearchState, FileViewerState};
use crate::theme::ThemePalette;
use crate::ui::ui_utils::TabScrollbarContext;
use lumis::highlight::Highlighter;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

/// Parse a hex color string like "#rrggbb" into a ratatui `Color::Rgb`.
fn parse_hex_color(hex: Option<&String>) -> Option<Color> {
    let hex = hex.as_ref()?;
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

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
    palette: &ThemePalette,
    borders: bool,
    icons_enabled: bool,
) {
    viewer.area = area;

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
    viewer.render_area = inner_area;
    f.render_widget(block, area);

    if viewer
        .protocol
        .as_ref()
        .and_then(|p| p.protocol_type())
        .is_some()
    {
        viewer.prepare_image_protocol();
        let resize = if viewer.is_image_zoomed() {
            ratatui_image::Resize::Scale(Some(ratatui_image::FilterType::CatmullRom))
        } else {
            ratatui_image::Resize::Fit(Some(ratatui_image::FilterType::CatmullRom))
        };
        if let Some(protocol) = &mut viewer.protocol {
            f.render_stateful_widget(
                ratatui_image::StatefulImage::new().resize(resize),
                inner_area,
                protocol,
            );
        }
        return;
    }

    if viewer.content.is_empty() && viewer.large_file_indexer.is_none() {
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

    render_text_content(f, viewer, area, inner_area, palette, borders, icons_enabled);
}

fn render_text_content(
    f: &mut ratatui::Frame,
    viewer: &mut FileViewerState,
    area: Rect,
    inner_area: Rect,
    palette: &ThemePalette,
    borders: bool,
    icons_enabled: bool,
) {
    let visible_lines = viewer.visible_lines();
    let (max_lines, is_large_file) = if let Some(indexer) = &viewer.large_file_indexer {
        (indexer.total_lines(), true)
    } else {
        (viewer.content.len(), false)
    };

    let max_scroll = max_lines.saturating_sub(visible_lines);
    let start_line = viewer.scroll_offset.min(max_scroll);
    let end_line = (start_line + visible_lines).min(max_lines);

    let highlighter = Highlighter::new(viewer.language, viewer.theme.clone());
    let default_fg = viewer
        .theme
        .as_ref()
        .and_then(|t| t.fg().map(std::string::ToString::to_string))
        .and_then(|s| parse_hex_color(Some(&s)));

    let normalized_selection = viewer.selection.map(normalize_selection);

    let mut lines = Vec::new();
    for i in start_line..end_line {
        if let Some(line) = render_viewer_line(&ViewerLineContext {
            line_idx: i,
            viewer,
            is_large_file,
            highlighter: &highlighter,
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

    let scroll_area = Rect {
        x: area.x + area.width - 1,
        y: area.y + 1,
        width: 1,
        height: area.height.saturating_sub(2),
    };

    crate::ui::ui_utils::draw_tab_scrollbar(
        f,
        scroll_area,
        max_lines,
        visible_lines,
        viewer.scroll_offset,
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
            input_value: &state.query,
            cursor_position: state.cursor_position,
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
    highlighter: &'a Highlighter,
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
    let dir_color = Color::Rgb(ctx.palette.blue.r, ctx.palette.blue.g, ctx.palette.blue.b);
    let text_color = Color::Rgb(ctx.palette.text.r, ctx.palette.text.g, ctx.palette.text.b);
    let subtext_color = Color::Rgb(
        ctx.palette.subtext.r,
        ctx.palette.subtext.g,
        ctx.palette.subtext.b,
    );

    let icon = if ctx.icons_enabled {
        crate::icons::get_icon(&row.name, row.is_dir, false)
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
        .current_search_match
        .filter(|(line, _, _)| *line == ctx.line_idx)
        .map(|(_, s, e)| (map_content_to_rendered(s), map_content_to_rendered(e)))
        .into_iter()
        .collect();

    let render_segment = |text: &str, seg_start: usize, style: Style| -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (pos, ch) in (seg_start..).zip(text.chars()) {
            let in_range = search_ranges.iter().any(|(s, e)| pos >= *s && pos < *e);
            let char_style = if in_range {
                style.fg(search_fg).bg(highlight_bg)
            } else {
                style
            };
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
    };

    let mut spans = Vec::new();
    let mut seg_start = 0;
    let prefix_style = Style::default().fg(overlay_color);
    let entry_style = Style::default().fg(if row.is_dir { dir_color } else { text_color });

    spans.extend(render_segment(&row.prefix, seg_start, prefix_style));
    seg_start += prefix_chars;
    if !icon.is_empty() {
        spans.extend(render_segment(&format!("{icon} "), seg_start, entry_style));
        seg_start += icon_chars + 1;
    }
    spans.extend(render_segment(&row.name, seg_start, entry_style));

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

fn render_viewer_line(ctx: &ViewerLineContext) -> Option<Line<'static>> {
    if let Some(rows) = &ctx.viewer.archive_rows
        && let Some(row) = rows.get(ctx.line_idx)
    {
        return Some(render_archive_line(ctx, row));
    }

    let selection_range = ctx.normalized_selection.and_then(|((r1, c1), (r2, c2))| {
        if ctx.line_idx < r1 || ctx.line_idx > r2 {
            None
        } else if ctx.line_idx > r1 && ctx.line_idx < r2 {
            Some((0, usize::MAX))
        } else if r1 == r2 {
            Some((c1, c2))
        } else if ctx.line_idx == r1 {
            Some((c1, usize::MAX))
        } else {
            Some((0, c2))
        }
    });

    let line_content = if ctx.is_large_file {
        ctx.viewer.large_file_indexer.as_ref().and_then(|indexer| {
            ctx.viewer.large_file_reader.as_ref().and_then(|reader| {
                indexer
                    .get_line_with_reader(ctx.line_idx, reader)
                    .map(|(s, e)| reader.get_chunk(s, e))
            })
        })
    } else {
        ctx.viewer.content.get(ctx.line_idx).cloned()
    };

    let line_content = line_content?;

    let search_ranges: Vec<(usize, usize)> = ctx
        .viewer
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

    let spans = if ctx.is_large_file {
        let style = lumis::themes::Style::default();
        generate_line_spans(&LineSpansContext {
            ranges: vec![(&style, line_content.as_str())],
            h_offset: ctx.viewer.horizontal_scroll_offset,
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
        })
    } else {
        let segments = ctx.highlighter.highlight(&line_content).unwrap_or_default();
        let ranges: Vec<(&lumis::themes::Style, &str)> = segments
            .iter()
            .map(|(style, text)| (style.as_ref(), *text))
            .collect();

        generate_line_spans(&LineSpansContext {
            ranges,
            h_offset: ctx.viewer.horizontal_scroll_offset,
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
        })
    };

    Some(Line::from(spans))
}

/// Generates spans for a single line, handling horizontal scrolling and width constraints
/// taking into account tab widths and wide characters.
#[must_use]
pub struct LineSpansContext<'a> {
    pub ranges: Vec<(&'a lumis::themes::Style, &'a str)>,
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

    fn process_char(
        &self,
        ch: char,
        current_pos: usize,
        _char_idx: usize,
        visible_width: usize,
    ) -> Option<(String, usize)> {
        let ch_width = if ch == '\t' {
            4 - (current_pos % 4)
        } else {
            unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
        };

        let ch_end_pos = current_pos + ch_width;
        if ch_end_pos <= self.h_offset || current_pos >= self.h_offset + self.max_width {
            return None;
        }

        let mut text = String::new();
        let mut width = 0;

        if current_pos >= self.h_offset {
            if visible_width + ch_width <= self.max_width {
                if ch == '\t' {
                    for _ in 0..ch_width {
                        text.push(' ');
                    }
                } else {
                    text.push(ch);
                }
                width = ch_width;
            }
        } else if ch == '\t' {
            let visible_tab_width = ch_end_pos - self.h_offset;
            if visible_width + visible_tab_width <= self.max_width {
                for _ in 0..visible_tab_width {
                    text.push(' ');
                }
                width = visible_tab_width;
            }
        }

        if text.is_empty() {
            None
        } else {
            Some((text, width))
        }
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
    let mut spans: Vec<Span> = Vec::new();
    let mut char_idx = 0;

    for (style, text) in &ctx.ranges {
        if visible_width >= ctx.max_width {
            break;
        }

        let text_width = LineSpansContext::calculate_text_display_width(text, display_pos);
        let end_pos = display_pos + text_width;

        if end_pos > ctx.h_offset {
            let mut current_pos = display_pos;
            for ch in text.chars() {
                if let Some((char_text, char_width)) =
                    ctx.process_char(ch, current_pos, char_idx, visible_width)
                {
                    let base_fg = parse_hex_color(Option::from(&style.fg)).or(ctx.default_fg);
                    let ratatui_style = ctx.get_char_style(char_idx, base_fg);

                    if let Some(last_span) = spans.last_mut()
                        && last_span.style == ratatui_style
                    {
                        let mut new_content = last_span.content.to_string();
                        new_content.push_str(&char_text);
                        last_span.content = new_content.into();
                    } else {
                        spans.push(Span::styled(char_text, ratatui_style));
                    }
                    visible_width += char_width;
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
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::archive::preview::ArchiveTreeRow;
    use crate::theme::catppuccin_macchiato;
    use lumis::themes::Style as LumisStyle;
    use unicode_width::UnicodeWidthStr;

    fn test_palette() -> crate::theme::ThemePalette {
        catppuccin_macchiato()
    }

    #[test]
    fn test_archive_search_highlight() {
        let mut viewer = FileViewerState::new(true, "catppuccin macchiato");
        viewer.archive_rows = Some(vec![ArchiveTreeRow {
            prefix: "└─ ".to_string(),
            name: "src".to_string(),
            is_dir: true,
            size: None,
        }]);
        // Content line is "└─ src"; match "src" at chars 3-6.
        viewer.current_search_match = Some((0, 3, 6));

        let ctx = ViewerLineContext {
            line_idx: 0,
            viewer: &viewer,
            is_large_file: false,
            highlighter: &Highlighter::new(lumis::languages::Language::default(), None),
            default_fg: None,
            normalized_selection: None,
            max_width: 100,
            palette: &test_palette(),
            icons_enabled: true,
        };

        let line = render_viewer_line(&ctx).expect("line should render");
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "└─ 󰉋 src");

        // The highlighted "src" span should have yellow bg and base fg.
        let highlighted = line
            .spans
            .iter()
            .find(|s| s.content == "src")
            .expect("src span");
        assert_eq!(
            highlighted.style.bg,
            Some(Color::Rgb(
                test_palette().yellow.r,
                test_palette().yellow.g,
                test_palette().yellow.b
            ))
        );
        assert_eq!(
            highlighted.style.fg,
            Some(Color::Rgb(
                test_palette().base.r,
                test_palette().base.g,
                test_palette().base.b
            ))
        );
    }

    #[test]
    fn test_archive_line_hides_icons_when_disabled() {
        let mut viewer = FileViewerState::new(true, "catppuccin macchiato");
        viewer.archive_rows = Some(vec![ArchiveTreeRow {
            prefix: "└─ ".to_string(),
            name: "main.rs".to_string(),
            is_dir: false,
            size: Some(1024),
        }]);

        let ctx = ViewerLineContext {
            line_idx: 0,
            viewer: &viewer,
            is_large_file: false,
            highlighter: &Highlighter::new(lumis::languages::Language::default(), None),
            default_fg: None,
            normalized_selection: None,
            max_width: 100,
            palette: &test_palette(),
            icons_enabled: false,
        };

        let line = render_viewer_line(&ctx).expect("line should render");
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.starts_with("└─ main.rs"), "got: {text}");
        assert!(!line.spans.iter().any(|s| s.content == "󰉋"));
    }

    #[test]
    fn test_rendering_overflow_prevention() {
        // Simulate a viewport width
        let max_width = 10;
        let h_offset = 0;

        // Dummy style for testing
        let dummy_style = LumisStyle {
            fg: Some("#ffffff".to_string()),
            ..Default::default()
        };

        // Test cases that would overflow if tabs were counted as 1 char but rendered as 4 spaces
        // or if unicode width wasn't handled correctly.
        let test_cases = vec![
            // Case 1: Tabs expanding.
            // '\t' (4 spaces) + '\t' (4 spaces) + "ABC" (3 chars) = 11 width.
            // Char count = 5. If we only checked char count (5 < 10), this would overflow.
            ("\t\tABC", "Two tabs and text"),
            // Case 2: Mixed content just over the limit
            // "1234567890" (10 chars) + "1" = 11 width.
            ("12345678901", "Simple overflow"),
            // Case 3: Tab crossing the boundary
            // "12345678" (8 chars) + "\t" (4 chars -> pos 12).
            ("12345678\t", "Tab crossing boundary"),
        ];

        for (input, description) in test_cases {
            // Treat the whole line as one range for baseline testing
            let ranges = vec![(&dummy_style, input)];

            let spans = generate_line_spans(&LineSpansContext {
                ranges,
                h_offset,
                max_width,
                default_fg: None,
                selection: None,
                selection_bg: None,
                search_ranges: &[],
                search_bg: None,
                search_fg: None,
            });

            // Calculate total display width of the generated spans
            let mut total_width = 0;
            let mut resulting_text = String::new();
            for span in &spans {
                // Note: generate_line_spans converts tabs to spaces, so width() works here
                total_width += span.content.width();
                resulting_text.push_str(&span.content);
            }

            println!("Test Case: {description}");
            println!("  Input: {input:?}");
            println!("  Result: '{resulting_text}'");
            println!("  Display width: {total_width}");

            // Assert that the total width does not exceed max_width
            assert!(
                total_width <= max_width,
                "Overflow detected for '{description}'! Width: {total_width}, Max: {max_width}. Result: '{resulting_text}'"
            );
        }
    }
}
