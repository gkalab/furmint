// Utility functions for UI truncation, formatting, and generic UI widgets

use crate::app::Tab;
use crate::theme::ThemePalette;
use crate::ui::button_widget::{ButtonVariant, ButtonWidget};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::env;

/// Draws a horizontal row of button widgets
/// - `labels`: List of raw strings such as ["[Y]es", "(N)o"]
/// - `area`: Rect to render into
/// - `f`: Frame reference
/// - `palette`: Theme palette for coloring
/// - `bg_color`: Background color for the popup
/// - `focused_index`: Index of the button that is focused, if any
///
/// # Panics
/// Panics if button width or count exceeds `u16::MAX`.
/// Compute button rectangles for a row of buttons.
/// Uses the same layout logic as `draw_button_row`.
#[must_use]
pub fn compute_button_rects(labels: &[&str], area: Rect) -> Vec<Rect> {
    let mut max_len = 0;
    for label in labels {
        let (parsed, _, _) = parse_button_label(label);
        max_len = max_len.max(parsed.chars().count());
    }

    let btn_w = u16::try_from((max_len + 4).max(12)).expect("button width fits in u16");
    let gap = 2u16;
    let num_buttons = u16::try_from(labels.len()).expect("button count fits in u16");
    let total_w = area.width;
    let total_buttons_w = num_buttons * btn_w + (num_buttons.saturating_sub(1)) * gap;

    let (final_btn_w, final_gap) = if total_buttons_w > total_w {
        let min_gap = 1u16;
        let available_w = total_w.saturating_sub(num_buttons.saturating_sub(1) * min_gap);
        let w = available_w / num_buttons;
        (w, min_gap)
    } else {
        (btn_w, gap)
    };

    let actual_total_w = num_buttons * final_btn_w + (num_buttons.saturating_sub(1)) * final_gap;
    let x_off = (total_w.saturating_sub(actual_total_w)) / 2;

    (0..labels.len())
        .map(|i| Rect {
            x: area.x
                + x_off
                + u16::try_from(i).expect("loop index fits in u16") * (final_btn_w + final_gap),
            y: area.y,
            width: final_btn_w,
            height: area.height,
        })
        .collect()
}

pub fn draw_button_row(
    f: &mut ratatui::Frame<'_>,
    labels: &[&str],
    area: Rect,
    palette: &crate::theme::ThemePalette,
    bg_color: Color,
    focused_index: Option<usize>,
) {
    let button_areas = compute_button_rects(labels, area);

    let parsed_labels: Vec<(String, Option<char>, Option<usize>)> = labels
        .iter()
        .map(|label| parse_button_label(label))
        .collect();

    let variant = if palette.is_dark {
        ButtonVariant::Dark
    } else {
        ButtonVariant::Light
    };

    for (i, (label, shortcut, shortcut_pos)) in parsed_labels.into_iter().enumerate() {
        let btn_area = button_areas[i];

        let mut btn = ButtonWidget::new(&label, variant)
            .with_palette(palette)
            .outer_bg(bg_color);

        if let (Some(ch), Some(pos)) = (shortcut, shortcut_pos) {
            btn = btn.shortcut(ch, pos);
        }

        if Some(i) == focused_index {
            btn = btn.focused(true);
        }

        f.render_widget(btn, btn_area);
    }
}

fn parse_button_label(raw: &str) -> (String, Option<char>, Option<usize>) {
    let mut label = String::new();
    let mut shortcut = None;
    let mut shortcut_pos = None;
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if (chars[i] == '[' || chars[i] == '(')
            && i + 2 < chars.len()
            && (chars[i + 2] == ']' || chars[i + 2] == ')')
        {
            shortcut = Some(chars[i + 1]);
            shortcut_pos = Some(label.chars().count());
            label.push(chars[i + 1]);
            i += 3;
        } else {
            label.push(chars[i]);
            i += 1;
        }
    }
    (label, shortcut, shortcut_pos)
}

#[must_use]
pub fn truncate_middle_with_ellipsis(name: &str, max_width: usize) -> String {
    let ellipsis = "…"; // Unicode ellipsis
    let ellipsis_len = ellipsis.chars().count();
    let name_len = name.chars().count();
    if name_len <= max_width {
        return name.to_string();
    }
    if max_width <= ellipsis_len {
        return ellipsis.repeat(max_width);
    }
    let keep = max_width - ellipsis_len;
    let left = keep / 2;
    let right = keep - left;
    let left_str: String = name.chars().take(left).collect();
    let right_str: String = name.chars().skip(name_len - right).collect();
    let mut result = format!("{left_str}{ellipsis}{right_str}");
    let result_len = result.chars().count();
    if result_len > max_width {
        result = result.chars().take(max_width).collect();
    } else if result_len < max_width {
        result = format!("{result:<max_width$}");
    }
    result
}

/// Returns a display path with the user's home directory prefix replaced by `~`.
///
/// Paths not under the home directory (or when it cannot be determined) are
/// returned unchanged.
#[must_use]
pub fn replace_home_with_tilde(path: &std::path::Path) -> String {
    let Some(home) = std::env::home_dir() else {
        return path.to_string_lossy().to_string();
    };
    let Ok(rest) = path.strip_prefix(&home) else {
        return path.to_string_lossy().to_string();
    };
    if rest.as_os_str().is_empty() {
        return "~".to_string();
    }
    let mut display = std::path::PathBuf::from("~");
    display.push(rest);
    display.to_string_lossy().to_string()
}

#[must_use]
pub fn truncate_path_with_ellipsis(path: &std::path::Path, max_width: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.chars().count() <= max_width {
        return path_str.to_string();
    }

    let components: Vec<_> = path.components().collect();
    let total_components = components.len();

    if total_components == 0 {
        return String::new();
    }

    let ellipsis = "…";
    let mut left_count = 1;
    let mut right_count = 1;
    let mut last_result = String::new();

    // Iterate to find the best fit
    while left_count + right_count < total_components {
        let mut new_path = std::path::PathBuf::new();

        // Add left components
        for c in &components[..left_count] {
            new_path.push(c);
        }

        // Add ellipsis (as a component)
        new_path.push(ellipsis);

        // Add right components
        for c in &components[total_components.saturating_sub(right_count)..] {
            new_path.push(c);
        }

        let result = new_path.to_string_lossy().to_string();
        if result.chars().count() > max_width {
            break;
        }

        last_result = result;

        // Try to add more segments
        if left_count <= right_count {
            left_count += 1;
        } else {
            right_count += 1;
        }
    }

    // Fallback if nothing fits or initial split failed:
    // Truncate the whole string with ellipsis in the middle (using existing function)
    if last_result.is_empty() {
        // If we have components but couldn't fit even 1+1+ellipsis,
        // or if it was just 1 component that is too long.
        // fallback to string truncation
        return truncate_middle_with_ellipsis(&path_str, max_width);
    }

    last_result
}

/// Helper function to create a lighter shade of red for inactive borders
#[must_use]
pub fn lighten_red(red: crate::theme::Rgb) -> crate::theme::Rgb {
    crate::theme::Rgb::new(
        u8::try_from(u16::midpoint(u16::from(red.r), 255)).unwrap_or(255),
        u8::try_from(u16::midpoint(u16::from(red.g), 255)).unwrap_or(255),
        u8::try_from(u16::midpoint(u16::from(red.b), 255)).unwrap_or(255),
    )
}

/// Check if the current tab is accessing a root location
#[must_use]
pub fn is_root_user(tab: &Tab) -> bool {
    let system_user = env::var("USER").unwrap_or_default();
    tab.provider.display_prefix().starts_with("[root@")
        || (tab.provider.display_prefix().is_empty() && system_user == "root")
}

/// Calculate the background color for a panel based on active state and root status
#[must_use]
pub fn panel_bg_color(palette: &ThemePalette, active: bool, is_root: bool, borders: bool) -> Color {
    let base_bg = if active || borders {
        Color::Rgb(palette.base.r, palette.base.g, palette.base.b)
    } else if palette.is_dark {
        let r = u8::try_from((u16::from(palette.base.r) * 3 + u16::from(palette.surface1.r)) / 4)
            .unwrap_or(255);
        let g = u8::try_from((u16::from(palette.base.g) * 3 + u16::from(palette.surface1.g)) / 4)
            .unwrap_or(255);
        let b = u8::try_from((u16::from(palette.base.b) * 3 + u16::from(palette.surface1.b)) / 4)
            .unwrap_or(255);
        Color::Rgb(r, g, b)
    } else {
        let r = u8::try_from((u16::from(palette.base.r) * 14 + u16::from(palette.surface1.r)) / 15)
            .unwrap_or(255);
        let g = u8::try_from((u16::from(palette.base.g) * 14 + u16::from(palette.surface1.g)) / 15)
            .unwrap_or(255);
        let b = u8::try_from((u16::from(palette.base.b) * 14 + u16::from(palette.surface1.b)) / 15)
            .unwrap_or(255);
        Color::Rgb(r, g, b)
    };

    if is_root && !borders {
        let (r0, g0, b0) = match base_bg {
            Color::Rgb(r, g, b) => (u16::from(r), u16::from(g), u16::from(b)),
            _ => (
                u16::from(palette.base.r),
                u16::from(palette.base.g),
                u16::from(palette.base.b),
            ),
        };
        let r = u8::try_from((r0 * 9 + u16::from(palette.red.r)) / 10).unwrap_or(255);
        let g = u8::try_from((g0 * 9 + u16::from(palette.red.g)) / 10).unwrap_or(255);
        let b = u8::try_from((b0 * 9 + u16::from(palette.red.b)) / 10).unwrap_or(255);
        Color::Rgb(r, g, b)
    } else {
        base_bg
    }
}

/// Draws a vertical scrollbar.
///
/// `viewport_based` indicates whether `offset` is the first visible line/row of a
/// viewport (so the max offset is `content_length - visible_length` and the thumb
/// rests at the bottom when scrolled to the end) or a cursor/item index (max offset
/// `content_length - 1`).
pub fn draw_scrollbar(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
    palette: &crate::theme::ThemePalette,
    viewport_based: bool,
) {
    let scrollbar_color = Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);
    let track_color = scrollbar_color;
    let track_symbol = Some(" ");

    draw_scrollbar_impl(
        f,
        &ScrollbarContext {
            area,
            content_length,
            visible_length,
            offset,
        },
        track_color,
        scrollbar_color,
        track_symbol,
        viewport_based,
    );
}

/// Context for rendering tab scrollbar
pub struct TabScrollbarContext<'a> {
    pub palette: &'a ThemePalette,
    pub borders: bool,
    pub is_root: bool,
    pub active: bool,
}

pub fn draw_tab_scrollbar(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
    ctx: &TabScrollbarContext,
    viewport_based: bool,
) {
    let scrollbar_color = Color::Rgb(
        ctx.palette.overlay0.r,
        ctx.palette.overlay0.g,
        ctx.palette.overlay0.b,
    );
    let track_color = if ctx.is_root && ctx.active {
        Color::Rgb(ctx.palette.red.r, ctx.palette.red.g, ctx.palette.red.b)
    } else if ctx.is_root && !ctx.active {
        let light_red = lighten_red(ctx.palette.red);
        Color::Rgb(light_red.r, light_red.g, light_red.b)
    } else if ctx.active {
        Color::Rgb(
            ctx.palette.border.r,
            ctx.palette.border.g,
            ctx.palette.border.b,
        )
    } else {
        Color::Rgb(
            ctx.palette.overlay0.r,
            ctx.palette.overlay0.g,
            ctx.palette.overlay0.b,
        )
    };
    let track_symbol = if ctx.borders { Some("│") } else { Some(" ") };
    draw_scrollbar_impl(
        f,
        &ScrollbarContext {
            area,
            content_length,
            visible_length,
            offset,
        },
        track_color,
        scrollbar_color,
        track_symbol,
        viewport_based,
    );
}

struct ScrollbarContext {
    area: ratatui::layout::Rect,
    content_length: usize,
    visible_length: usize,
    offset: usize,
}

/// Integer division that rounds to the nearest integer (rounds up on ties),
/// matching ratatui's `rounding_divide` used for scrollbar part lengths.
/// Overflow-safe: never computes `numerator + denominator / 2`.
const fn rounding_divide(numerator: usize, denominator: usize) -> usize {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    // `remainder >= denominator - remainder` ⟺ `2 * remainder >= denominator`.
    // `denominator - remainder` cannot underflow because `remainder < denominator`.
    if remainder >= denominator - remainder {
        quotient + 1
    } else {
        quotient
    }
}

/// Computes the thumb geometry `(start, end)` (end exclusive) in track rows for a
/// vertical scrollbar. Shared by rendering (`draw_scrollbar_impl`) and hit-testing
/// (`crate::handlers::mouse::scrollbar_thumb_rows`) so the two never diverge.
///
/// When `viewport_based` is true the max offset is `content_length - visible_length`
/// (thumb rests at the bottom when scrolled to the end); otherwise it is
/// `content_length - 1` (cursor/item index).
///
/// Returns `None` when the scrollbar is not rendered (content fits within the viewport
/// or the track has zero height).
#[must_use]
pub fn scrollbar_thumb_geometry(
    track_length: usize,
    content_length: usize,
    visible_length: usize,
    offset: usize,
    viewport_based: bool,
) -> Option<(usize, usize)> {
    if content_length <= visible_length || track_length == 0 {
        return None;
    }
    let max_position = if viewport_based {
        content_length.saturating_sub(visible_length)
    } else {
        content_length.saturating_sub(1)
    };
    let start_position = offset.min(max_position);
    let max_viewport_position = max_position.saturating_add(visible_length);
    if max_viewport_position == 0 {
        return None;
    }

    let thumb_length = rounding_divide(
        visible_length.saturating_mul(track_length),
        max_viewport_position,
    )
    .clamp(1, track_length);

    let thumb_start = rounding_divide(
        start_position.saturating_mul(track_length),
        max_viewport_position,
    )
    .clamp(0, track_length.saturating_sub(thumb_length));

    Some((thumb_start, thumb_start + thumb_length))
}

fn draw_scrollbar_impl(
    f: &mut ratatui::Frame,
    ctx: &ScrollbarContext,
    track_color: Color,
    scrollbar_color: Color,
    track_symbol: Option<&str>,
    viewport_based: bool,
) {
    use ratatui::style::Style;
    use ratatui::widgets::Paragraph;

    let track_symbol = track_symbol.unwrap_or(" ");

    let track_length = ctx.area.height as usize;
    let Some((thumb_start, thumb_end)) = scrollbar_thumb_geometry(
        track_length,
        ctx.content_length,
        ctx.visible_length,
        ctx.offset,
        viewport_based,
    ) else {
        return;
    };

    let mut rows = Vec::with_capacity(track_length);
    for i in 0..track_length {
        let on_thumb = i >= thumb_start && i < thumb_end;
        rows.push(Line::from(Span::styled(
            if on_thumb { "▊" } else { track_symbol },
            Style::default().fg(if on_thumb {
                scrollbar_color
            } else {
                track_color
            }),
        )));
    }
    // Render in the rightmost column of the area, matching ratatui's
    // `ScrollbarOrientation::VerticalRight`.
    let bar_area = Rect {
        x: ctx.area.x + ctx.area.width.saturating_sub(1),
        y: ctx.area.y,
        width: 1,
        height: ctx.area.height,
    };
    f.render_widget(Paragraph::new(rows), bar_area);
}

#[must_use]
pub fn centered_rect_percent(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[must_use]
pub fn centered_rect_absolute(width: u16, height: u16, r: Rect) -> Rect {
    let popup_x = (r.width.saturating_sub(width)) / 2;
    let popup_y = (r.height.saturating_sub(height)) / 2;

    Rect {
        x: popup_x,
        y: popup_y,
        width,
        height,
    }
}

#[must_use]
pub fn field_border_set() -> ratatui::symbols::border::Set<'static> {
    ratatui::symbols::border::Set {
        top_left: "▎",
        top_right: " ",
        bottom_left: "▎",
        bottom_right: " ",
        vertical_left: "▎",
        vertical_right: " ",
        horizontal_top: " ",
        horizontal_bottom: " ",
    }
}

#[must_use]
pub fn message_border_set() -> ratatui::symbols::border::Set<'static> {
    ratatui::symbols::border::Set {
        top_left: "\u{2594}",
        top_right: "\u{2594}",
        bottom_left: " ",
        bottom_right: " ",
        vertical_left: " ",
        vertical_right: " ",
        horizontal_top: "\u{2594}",
        horizontal_bottom: " ",
    }
}

pub struct InputPopupOptions<'a> {
    pub title: Option<&'a str>,
    pub input_value: &'a str,
    pub cursor_position: usize,
    pub error: Option<&'a str>,
    pub placeholder: &'a str,
    pub width: u16,
}

pub struct InputLayoutContext {
    pub chunk: Rect,
    pub layout_rect: Rect,
    pub has_title: bool,
}

impl InputLayoutContext {
    #[must_use]
    pub fn new(chunk: Rect, layout_rect: Rect, has_title: bool) -> Self {
        Self {
            chunk,
            layout_rect,
            has_title,
        }
    }
}

pub struct InputStyleContext {
    pub text_color: Color,
    pub placeholder_color: Color,
    pub field_bg_color: Color,
}

impl InputStyleContext {
    #[must_use]
    pub fn new(text_color: Color, placeholder_color: Color, field_bg_color: Color) -> Self {
        Self {
            text_color,
            placeholder_color,
            field_bg_color,
        }
    }
}

pub fn draw_input_popup(
    f: &mut ratatui::Frame,
    options: &InputPopupOptions,
    palette: &crate::theme::ThemePalette,
) {
    use ratatui::widgets::{Block, Borders, Clear};

    let has_title = options.title.is_some();
    let popup_width = options.width;
    let popup_height = if has_title { 7 } else { 5 };
    let popup_area = centered_rect_absolute(popup_width, popup_height, f.area());

    // Clear the popup area
    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.mantle.r, palette.mantle.g, palette.mantle.b);
    let field_bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.border.r, palette.border.g, palette.border.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);
    let title_color = Color::Rgb(palette.blue.r, palette.blue.g, palette.blue.b);
    let error_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let placeholder_color = Color::Rgb(palette.overlay0.r, palette.overlay0.g, palette.overlay0.b);

    // Draw outer block (shadow/background)
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_set(ratatui::symbols::border::EMPTY)
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    // Inner chunks for vertical spacing
    let constraints = if has_title {
        vec![Constraint::Length(5)]
    } else {
        vec![Constraint::Length(3)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints(constraints)
        .split(popup_area);

    // Draw input block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color).bg(field_bg_color))
        .border_set(field_border_set())
        .style(Style::default().bg(field_bg_color));

    f.render_widget(&block, chunks[0]);

    // Layout inside padding for title, input, error
    let text_area = Rect {
        x: chunks[0].x + 2,
        y: chunks[0].y,
        width: chunks[0].width.saturating_sub(2),
        height: chunks[0].height,
    };

    let layout_constraints = if has_title {
        vec![
            Constraint::Length(1), // Spacing (top border)
            Constraint::Length(1), // Title
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Input
            Constraint::Length(1), // Error
        ]
    } else {
        vec![
            Constraint::Length(1), // Spacing (top border)
            Constraint::Length(1), // Input
            Constraint::Length(1), // Error
        ]
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(layout_constraints)
        .split(text_area);

    if let Some(t) = options.title {
        let title_paragraph = Paragraph::new(t)
            .style(Style::default().fg(title_color).bg(field_bg_color))
            .alignment(Alignment::Left);
        f.render_widget(title_paragraph, layout[1]);
    }

    let input_index = if has_title { 3 } else { 1 };
    let input_layout = InputLayoutContext::new(chunks[0], layout[input_index], has_title);
    let style = InputStyleContext::new(text_color, placeholder_color, field_bg_color);
    draw_input_text_and_cursor(f, options, &input_layout, &style);

    // Error
    let error_index = if has_title { 4 } else { 2 };
    if let Some(error) = options.error {
        let error_paragraph = Paragraph::new(error)
            .style(Style::default().fg(error_color).bg(field_bg_color))
            .alignment(Alignment::Right);
        f.render_widget(error_paragraph, layout[error_index]);
    }
}

pub fn draw_confirmation_popup(
    f: &mut ratatui::Frame,
    state: &crate::state::ConfirmationState,
    palette: &ThemePalette,
    width: u16,
    height: u16,
    bg_color: Color,
) {
    if !state.is_visible {
        return;
    }

    let popup_area = centered_rect_absolute(width, height + 1, f.area());
    f.render_widget(Clear, popup_area);

    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b);
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    f.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(border_color))
            .border_set(message_border_set())
            .style(Style::default().bg(bg_color)),
        popup_area,
    );

    let content_area = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(1),
    };

    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(3),
        ])
        .split(content_area);

    let message = if state.truncate {
        truncate_middle_with_ellipsis(&state.message, (width - 8) as usize)
    } else {
        state.message.clone()
    };

    f.render_widget(
        Paragraph::new(message)
            .style(Style::default().fg(text_color).bg(bg_color))
            .alignment(Alignment::Center),
        inner_layout[1],
    );

    let focused = state.selected_no.then_some(0).or(Some(1));
    draw_button_row(
        f,
        &["(N)o", "(Y)es"],
        inner_layout[2],
        palette,
        bg_color,
        focused,
    );
}

fn draw_input_text_and_cursor(
    f: &mut ratatui::Frame,
    options: &InputPopupOptions,
    layout: &InputLayoutContext,
    style: &InputStyleContext,
) {
    let input_width = (layout.chunk.width as usize).saturating_sub(4);
    let cursor_pos = options.cursor_position;

    let scroll_offset = if cursor_pos < input_width {
        0
    } else {
        cursor_pos - input_width + 1
    };

    let display_text = if options.input_value.is_empty() {
        Span::styled(
            options.placeholder,
            Style::default().fg(style.placeholder_color),
        )
    } else {
        let text: String = options
            .input_value
            .chars()
            .skip(scroll_offset)
            .take(input_width)
            .collect();
        Span::styled(
            text,
            Style::default()
                .fg(style.text_color)
                .bg(style.field_bg_color),
        )
    };

    let paragraph = Paragraph::new(display_text).style(Style::default().bg(style.field_bg_color));

    f.render_widget(paragraph, layout.layout_rect);

    // Cursor
    let cursor_visual_offset = cursor_pos.saturating_sub(scroll_offset);
    if cursor_visual_offset < input_width {
        let y_offset = if layout.has_title { 3 } else { 1 };
        f.set_cursor_position(Position::new(
            layout.chunk.x + 2 + u16::try_from(cursor_visual_offset).unwrap_or(u16::MAX),
            layout.chunk.y + y_offset,
        ));
    }
}
