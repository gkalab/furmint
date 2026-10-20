use super::panel::lighten_red;
use crate::theme::ThemePalette;
use ratatui::prelude::*;

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
