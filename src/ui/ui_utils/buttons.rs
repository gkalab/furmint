use crate::ui::button_widget::{ButtonVariant, ButtonWidget};
use ratatui::prelude::*;

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

/// Draws a button row at pre-computed areas.
///
/// Callers that have a [`crate::popup_layout::ButtonGeometry`] use this so the
/// rendered buttons sit exactly where mouse hit-testing will look for them.
pub fn draw_button_row_at(
    f: &mut ratatui::Frame<'_>,
    labels: &[&str],
    button_areas: &[Rect],
    palette: &crate::theme::ThemePalette,
    bg_color: Color,
    focused_index: Option<usize>,
) {
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
        let Some(btn_area) = button_areas.get(i).copied() else {
            break;
        };

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
