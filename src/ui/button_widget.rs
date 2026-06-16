use crate::theme::ThemePalette;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Light,
    Dark,
}

pub struct ButtonWidget<'a> {
    label: &'a str,
    variant: ButtonVariant,
    is_hovered: bool,
    is_pressed: bool,
    is_focused: bool,
    shortcut: Option<char>,
    shortcut_pos: Option<usize>,
    palette: Option<&'a ThemePalette>,
    outer_bg: Option<Color>,
}

struct ButtonColors {
    body_bg: Color,
    text_fg: Color,
    top_bevel: Color,
    bottom_bevel: Color,
    shortcut_color: Color,
}

fn add(c: (u8, u8, u8), amount: u8) -> (u8, u8, u8) {
    (
        c.0.saturating_add(amount),
        c.1.saturating_add(amount),
        c.2.saturating_add(amount),
    )
}

fn sub(c: (u8, u8, u8), amount: u8) -> (u8, u8, u8) {
    (
        c.0.saturating_sub(amount),
        c.1.saturating_sub(amount),
        c.2.saturating_sub(amount),
    )
}

impl<'a> ButtonWidget<'a> {
    #[must_use]
    pub fn new(label: &'a str, variant: ButtonVariant) -> Self {
        Self {
            label,
            variant,
            is_hovered: false,
            is_pressed: false,
            is_focused: false,
            shortcut: None,
            shortcut_pos: None,
            palette: None,
            outer_bg: None,
        }
    }

    #[must_use]
    pub fn hovered(mut self, hovered: bool) -> Self {
        self.is_hovered = hovered;
        self
    }

    #[must_use]
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.is_pressed = pressed;
        self
    }

    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.is_focused = focused;
        self
    }

    #[must_use]
    pub fn shortcut(mut self, ch: char, pos: usize) -> Self {
        self.shortcut = Some(ch);
        self.shortcut_pos = Some(pos);
        self
    }

    #[must_use]
    pub fn outer_bg(mut self, bg: Color) -> Self {
        self.outer_bg = Some(bg);
        self
    }

    #[must_use]
    pub fn with_palette(mut self, palette: &'a ThemePalette) -> Self {
        self.palette = Some(palette);
        self
    }

    fn get_colors(&self) -> ButtonColors {
        let Some(pal) = self.palette else {
            let gray = match self.variant {
                ButtonVariant::Dark => (60, 60, 65),
                ButtonVariant::Light => (180, 185, 195),
            };
            let fg = match self.variant {
                ButtonVariant::Dark => (240, 240, 245),
                ButtonVariant::Light => (20, 20, 30),
            };
            let hovered = self.is_hovered || self.is_focused;
            let (body, text, top, bottom) = if hovered {
                (add(gray, 15), fg, add(gray, 45), add(gray, 0))
            } else {
                (gray, fg, add(gray, 40), sub(gray, 35))
            };
            return if self.is_pressed {
                let body = sub(body, 35);
                let text = sub(text, 40);
                ButtonColors {
                    body_bg: Color::Rgb(body.0, body.1, body.2),
                    text_fg: Color::Rgb(text.0, text.1, text.2),
                    top_bevel: Color::Rgb(bottom.0, bottom.1, bottom.2),
                    bottom_bevel: Color::Rgb(top.0, top.1, top.2),
                    shortcut_color: Color::Rgb(255, 230, 100),
                }
            } else {
                ButtonColors {
                    body_bg: Color::Rgb(body.0, body.1, body.2),
                    text_fg: Color::Rgb(text.0, text.1, text.2),
                    top_bevel: Color::Rgb(top.0, top.1, top.2),
                    bottom_bevel: Color::Rgb(bottom.0, bottom.1, bottom.2),
                    shortcut_color: Color::Rgb(255, 230, 100),
                }
            };
        };

        let s0 = (pal.mantle.r, pal.mantle.g, pal.mantle.b);
        let s0_hov = (pal.surface0.r, pal.surface0.g, pal.surface0.b);
        let txt = (pal.text.r, pal.text.g, pal.text.b);
        let hovered = self.is_hovered || self.is_focused;

        let (body_bg, text_fg, top_bevel, bottom_bevel) = if hovered {
            (s0_hov, txt, add(s0_hov, 15), sub(s0_hov, 20))
        } else {
            (s0, txt, add(s0, 10), sub(s0, 10))
        };

        let shortcut_color = Color::Rgb(pal.yellow.r, pal.yellow.g, pal.yellow.b);

        if self.is_pressed {
            let body = sub(body_bg, 35);
            let fg = sub(text_fg, 40);
            ButtonColors {
                body_bg: Color::Rgb(body.0, body.1, body.2),
                text_fg: Color::Rgb(fg.0, fg.1, fg.2),
                top_bevel: Color::Rgb(bottom_bevel.0, bottom_bevel.1, bottom_bevel.2),
                bottom_bevel: Color::Rgb(top_bevel.0, top_bevel.1, top_bevel.2),
                shortcut_color,
            }
        } else {
            ButtonColors {
                body_bg: Color::Rgb(body_bg.0, body_bg.1, body_bg.2),
                text_fg: Color::Rgb(text_fg.0, text_fg.1, text_fg.2),
                top_bevel: Color::Rgb(top_bevel.0, top_bevel.1, top_bevel.2),
                bottom_bevel: Color::Rgb(bottom_bevel.0, bottom_bevel.1, bottom_bevel.2),
                shortcut_color,
            }
        }
    }
}

impl Widget for ButtonWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height < 3 {
            return;
        }

        let colors = self.get_colors();
        let outer_bg = self.outer_bg.unwrap_or(colors.body_bg);

        for x in 0..area.width {
            let cell = &mut buf[(area.x + x, area.y)];
            cell.set_char('\u{2581}');
            cell.set_fg(colors.top_bevel);
            cell.set_bg(outer_bg);
        }

        for x in 0..area.width {
            let cell = &mut buf[(area.x + x, area.y + 1)];
            cell.set_char(' ');
            cell.set_bg(colors.body_bg);
        }

        let text_len = u16::try_from(self.label.chars().count()).unwrap_or(u16::MAX);
        let start_x = if area.width > text_len {
            area.x + (area.width - text_len) / 2
        } else {
            area.x
        };

        let max_x = area.x + area.width;

        for (i, (current_x, ch)) in (0..).zip((start_x..).zip(self.label.chars())) {
            if current_x >= max_x {
                break;
            }
            let cell = &mut buf[(current_x, area.y + 1)];
            cell.set_char(ch);
            cell.set_bg(colors.body_bg);

            if self.shortcut_pos == Some(i) {
                cell.set_fg(colors.shortcut_color);
                cell.set_style(Style::default().add_modifier(Modifier::BOLD));
            } else {
                cell.set_fg(colors.text_fg);
                if self.is_focused {
                    cell.set_style(Style::default().add_modifier(Modifier::BOLD));
                }
            }
        }

        for x in 0..area.width {
            let cell = &mut buf[(area.x + x, area.y + 2)];
            cell.set_char('\u{2594}');
            cell.set_fg(colors.bottom_bevel);
            cell.set_bg(outer_bg);
        }
    }
}
