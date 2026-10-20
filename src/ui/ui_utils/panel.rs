use crate::app_state::tabs::Tab;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use std::env;

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
