use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use directories::ProjectDirs;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct KeyboardConfig {
    pub history_previous: Option<Vec<String>>,
    pub history_next: Option<Vec<String>>,
    // Add more actions as needed
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ThemeConfig {
    pub name: Option<String>,
    // Add more theme fields as needed
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AppConfig {
    pub keyboard: Option<KeyboardConfig>,
    pub theme: Option<ThemeConfig>,
}

// Default key bindings (update as needed)
pub fn default_keyboard_config() -> KeyboardConfig {
    KeyboardConfig {
        history_previous: Some(vec!["Alt-Left".to_string()]),
        history_next: Some(vec!["Alt-Right".to_string()]),
        // Add more actions and their default shortcuts here
    }
}

pub fn default_theme_config() -> ThemeConfig {
    ThemeConfig {
        name: Some("mariana".to_string()),
        // Add more theme defaults here
    }
}

pub fn merge_keyboard_config(user: &Option<KeyboardConfig>, default: &KeyboardConfig) -> KeyboardConfig {
    KeyboardConfig {
        history_previous: user.as_ref().and_then(|k| k.history_previous.clone()).or_else(|| default.history_previous.clone()),
        history_next: user.as_ref().and_then(|k| k.history_next.clone()).or_else(|| default.history_next.clone()),
        // Add more actions as needed
    }
}

pub fn merge_theme_config(user: &Option<ThemeConfig>, default: &ThemeConfig) -> ThemeConfig {
    ThemeConfig {
        name: user.as_ref().and_then(|t| t.name.clone()).or_else(|| default.name.clone()),
        // Add more theme fields as needed
    }
}

pub fn load_config() -> Result<(KeyboardConfig, ThemeConfig), String> {
    let path = config_path().ok_or("Could not determine config directory")?;
    let default_keyboard = default_keyboard_config();
    let default_theme = default_theme_config();
    if path.exists() {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        let user_config: AppConfig = toml::from_str(&content)
            .map_err(|e| format!("Config file is invalid: {}", e))?;
        let keyboard = merge_keyboard_config(&user_config.keyboard, &default_keyboard);
        let theme = merge_theme_config(&user_config.theme, &default_theme);
        Ok((keyboard, theme))
    } else {
        Ok((default_keyboard, default_theme))
    }
}

pub fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "fm", "fm")
        .map(|proj_dirs| proj_dirs.config_dir().join("config.toml"))
}

