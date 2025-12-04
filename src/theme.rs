//! Theme definitions for the file manager.
//! 
//! Provides color palettes for different themes.

/// RGB color values
#[derive(Clone, Copy)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Color palette for a theme
#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub base: Rgb,       // Background
    pub mantle: Rgb,     // Darker background
    pub surface0: Rgb,   // Surface
    pub surface1: Rgb,   // Surface variant
    pub surface2: Rgb,   // Highlight background
    pub overlay0: Rgb,   // Muted/overlay
    pub text: Rgb,       // Primary text
    pub subtext: Rgb,    // Secondary text
    pub blue: Rgb,       // Directories
    pub green: Rgb,      // Executables
    pub yellow: Rgb,     // Headers
    pub red: Rgb,        // Errors
    pub is_dark: bool,   // Dark theme flag
}

// ============================================================================
// Catppuccin Themes (using catppuccin crate values)
// ============================================================================

pub fn catppuccin_macchiato() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(36, 39, 58),
        mantle: Rgb::new(30, 32, 48),
        surface0: Rgb::new(54, 58, 79),
        surface1: Rgb::new(65, 69, 89),
        surface2: Rgb::new(91, 96, 120),
        overlay0: Rgb::new(110, 115, 141),
        text: Rgb::new(202, 211, 245),
        subtext: Rgb::new(184, 192, 224),
        blue: Rgb::new(138, 173, 244),
        green: Rgb::new(166, 218, 149),
        yellow: Rgb::new(238, 212, 159),
        red: Rgb::new(237, 135, 150),
        is_dark: true,
    }
}

pub fn catppuccin_latte() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(239, 241, 245),
        mantle: Rgb::new(230, 233, 239),
        surface0: Rgb::new(204, 208, 218),
        surface1: Rgb::new(188, 192, 204),
        surface2: Rgb::new(172, 176, 190),
        overlay0: Rgb::new(156, 160, 176),
        text: Rgb::new(76, 79, 105),
        subtext: Rgb::new(92, 95, 119),
        blue: Rgb::new(30, 102, 245),
        green: Rgb::new(64, 160, 43),
        yellow: Rgb::new(223, 142, 29),
        red: Rgb::new(210, 15, 57),
        is_dark: false,
    }
}

pub fn catppuccin_frappe() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(48, 52, 70),
        mantle: Rgb::new(41, 44, 60),
        surface0: Rgb::new(65, 69, 89),
        surface1: Rgb::new(81, 87, 109),
        surface2: Rgb::new(98, 104, 128),
        overlay0: Rgb::new(115, 121, 148),
        text: Rgb::new(198, 208, 245),
        subtext: Rgb::new(181, 191, 226),
        blue: Rgb::new(140, 170, 238),
        green: Rgb::new(166, 209, 137),
        yellow: Rgb::new(229, 200, 144),
        red: Rgb::new(231, 130, 132),
        is_dark: true,
    }
}

pub fn catppuccin_mocha() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(30, 30, 46),
        mantle: Rgb::new(24, 24, 37),
        surface0: Rgb::new(49, 50, 68),
        surface1: Rgb::new(69, 71, 90),
        surface2: Rgb::new(88, 91, 112),
        overlay0: Rgb::new(108, 112, 134),
        text: Rgb::new(205, 214, 244),
        subtext: Rgb::new(186, 194, 222),
        blue: Rgb::new(137, 180, 250),
        green: Rgb::new(166, 227, 161),
        yellow: Rgb::new(249, 226, 175),
        red: Rgb::new(243, 139, 168),
        is_dark: true,
    }
}

// ============================================================================
// Dracula Theme
// ============================================================================

pub fn dracula() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(40, 42, 54),       // Background
        mantle: Rgb::new(33, 34, 44),     // Current Line (darker)
        surface0: Rgb::new(68, 71, 90),   // Comment (surface)
        surface1: Rgb::new(68, 71, 90),
        surface2: Rgb::new(98, 114, 164), // Selection
        overlay0: Rgb::new(98, 114, 164), // Comment
        text: Rgb::new(248, 248, 242),    // Foreground
        subtext: Rgb::new(189, 147, 249), // Purple (subtext)
        blue: Rgb::new(139, 233, 253),    // Cyan (directories)
        green: Rgb::new(80, 250, 123),    // Green (executables)
        yellow: Rgb::new(241, 250, 140),  // Yellow (headers)
        red: Rgb::new(255, 85, 85),       // Red (errors)
        is_dark: true,
    }
}

// ============================================================================
// Nord Theme
// ============================================================================

pub fn nord() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(46, 52, 64),       // nord0 - Polar Night
        mantle: Rgb::new(59, 66, 82),     // nord1
        surface0: Rgb::new(67, 76, 94),   // nord2
        surface1: Rgb::new(76, 86, 106),  // nord3
        surface2: Rgb::new(76, 86, 106),  // nord3
        overlay0: Rgb::new(216, 222, 233),// nord4 - Snow Storm (muted)
        text: Rgb::new(236, 239, 244),    // nord6 - Snow Storm
        subtext: Rgb::new(229, 233, 240), // nord5
        blue: Rgb::new(136, 192, 208),    // nord8 - Frost (directories)
        green: Rgb::new(163, 190, 140),   // nord14 - Aurora (executables)
        yellow: Rgb::new(235, 203, 139),  // nord13 - Aurora (headers)
        red: Rgb::new(191, 97, 106),      // nord11 - Aurora (errors)
        is_dark: true,
    }
}

// ============================================================================
// Solarized Light Theme
// ============================================================================

pub fn solarized_light() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(253, 246, 227),    // base3 - Background
        mantle: Rgb::new(238, 232, 213),  // base2 - Background highlights
        surface0: Rgb::new(238, 232, 213),// base2
        surface1: Rgb::new(147, 161, 161),// base1 - Comments
        surface2: Rgb::new(131, 148, 150),// base0 - Selection bg
        overlay0: Rgb::new(88, 110, 117), // base01 - Emphasis
        text: Rgb::new(101, 123, 131),    // base00 - Body text
        subtext: Rgb::new(88, 110, 117),  // base01
        blue: Rgb::new(38, 139, 210),     // blue (directories)
        green: Rgb::new(133, 153, 0),     // green (executables)
        yellow: Rgb::new(181, 137, 0),    // yellow (headers)
        red: Rgb::new(220, 50, 47),       // red (errors)
        is_dark: false,
    }
}

// ============================================================================
// Mariana Theme
// ============================================================================

pub fn mariana() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(48, 56, 65),        // Background #303841
        mantle: Rgb::new(38, 45, 53),      // Darker background
        surface0: Rgb::new(60, 70, 82),    // Surface
        surface1: Rgb::new(75, 88, 103),   // Surface variant
        surface2: Rgb::new(90, 105, 122),  // Selection #5a6978
        overlay0: Rgb::new(160, 166, 175), // Comment color #a0a6af
        text: Rgb::new(213, 220, 230),     // Foreground #d5dce6
        subtext: Rgb::new(180, 188, 198),  // Secondary text
        blue: Rgb::new(102, 175, 224),     // Blue #66afe0 (directories)
        green: Rgb::new(153, 199, 148),    // Green #99c794 (executables)
        yellow: Rgb::new(250, 200, 99),    // Yellow #fac863 (headers)
        red: Rgb::new(236, 95, 103),       // Red #ec5f67 (errors)
        is_dark: true,
    }
}

// ============================================================================
// Breakers Theme
// ============================================================================

pub fn breakers() -> ThemePalette {
    ThemePalette {
        base: Rgb::new(248, 248, 248),     // Light background #f8f8f8
        mantle: Rgb::new(238, 238, 238),   // Slightly darker #eeeeee
        surface0: Rgb::new(228, 228, 228), // Surface
        surface1: Rgb::new(210, 210, 210), // Surface variant
        overlay0: Rgb::new(190, 210, 225), // Selection (light blue tint)
        surface2: Rgb::new(120, 130, 140), // Comment/muted
        text: Rgb::new(48, 56, 65),        // Dark text (Mariana's bg)
        subtext: Rgb::new(80, 90, 100),    // Secondary text
        blue: Rgb::new(53, 124, 176),      // Blue (directories)
        green: Rgb::new(85, 145, 85),      // Green (executables)
        yellow: Rgb::new(180, 140, 40),    // Yellow/orange (headers)
        red: Rgb::new(200, 60, 70),        // Red (errors)
        is_dark: false,
    }
}

/// Get theme palette by name
pub fn get_theme(name: &str) -> Option<ThemePalette> {
    match name {
        "catppuccin macchiato" => Some(catppuccin_macchiato()),
        "catppuccin latte" => Some(catppuccin_latte()),
        "catppuccin frappe" => Some(catppuccin_frappe()),
        "catppuccin mocha" => Some(catppuccin_mocha()),
        "dracula" => Some(dracula()),
        "nord" => Some(nord()),
        "solarized light" => Some(solarized_light()),
        "mariana" => Some(mariana()),
        "breakers" => Some(breakers()),
        _ => None,
    }
}

/// Get default theme
pub fn default_theme() -> ThemePalette {
    catppuccin_macchiato()
}

/// List of all available theme names
pub const THEME_NAMES: &[&str] = &[
    "catppuccin macchiato",
    "catppuccin latte",
    "catppuccin frappe",
    "catppuccin mocha",
    "dracula",
    "nord",
    "solarized light",
    "mariana",
    "breakers",
];

