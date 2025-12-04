use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use directories::ProjectDirs;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct KeyboardConfig {
    pub previous_directory: Option<Vec<String>>,
    pub next_directory: Option<Vec<String>>,
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
        previous_directory: Some(vec!["Ctrl-Up".to_string(), "Left".to_string()]),
        next_directory: Some(vec!["Ctrl-Down".to_string(), "Right".to_string()]),
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
        previous_directory: user.as_ref().and_then(|k| k.previous_directory.clone()).or_else(|| default.previous_directory.clone()),
        next_directory: user.as_ref().and_then(|k| k.next_directory.clone()).or_else(|| default.next_directory.clone()),
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

