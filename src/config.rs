use directories::ProjectDirs;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct KeyboardConfig {
    pub history_previous: Option<Vec<String>>,
    pub history_next: Option<Vec<String>>,
    pub enter_directory: Option<Vec<String>>,
    pub directory_up: Option<Vec<String>>,
    pub edit: Option<Vec<String>>,
    pub new_tab: Option<Vec<String>>,
    pub next_tab: Option<Vec<String>>,
    pub prev_tab: Option<Vec<String>>,
    pub close_tab: Option<Vec<String>>,
    pub fuzzy_search: Option<Vec<String>>,
    pub sort_by_name: Option<Vec<String>>,
    pub sort_by_extension: Option<Vec<String>>,
    pub sort_by_date: Option<Vec<String>>,
    pub sort_by_size: Option<Vec<String>>,
    pub rename: Option<Vec<String>>,
    pub delete: Option<Vec<String>>,
    pub delete_permanently: Option<Vec<String>>,
    pub task_manager: Option<Vec<String>>,
    pub select_all: Option<Vec<String>>,
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
        enter_directory: Some(vec!["Right".to_string()]),
        directory_up: Some(vec!["Backspace".to_string(), "Left".to_string()]),
        edit: Some(vec!["F4".to_string()]),
        new_tab: Some(vec!["Ctrl-t".to_string()]),
        next_tab: Some(vec!["Ctrl-Right".to_string()]),
        prev_tab: Some(vec!["Ctrl-Left".to_string()]),
        close_tab: Some(vec!["Ctrl-w".to_string()]),
        fuzzy_search: Some(vec!["Ctrl-p".to_string()]),
        sort_by_name: Some(vec!["Ctrl-F2".to_string()]),
        sort_by_extension: Some(vec!["Ctrl-F4".to_string()]),
        sort_by_date: Some(vec!["Ctrl-F5".to_string()]),
        sort_by_size: Some(vec!["Ctrl-F6".to_string()]),
        rename: Some(vec!["F2".to_string()]),
        delete: Some(vec!["Delete".to_string()]),
        delete_permanently: Some(vec!["Shift-Delete".to_string()]),
        task_manager: Some(vec!["F10".to_string()]),
        select_all: Some(vec!["Ctrl-a".to_string()]),
    }
}

pub fn default_theme_config() -> ThemeConfig {
    ThemeConfig {
        name: Some("mariana".to_string()),
        // Add more theme defaults here
    }
}

pub fn merge_keyboard_config(
    user: &Option<KeyboardConfig>,
    default: &KeyboardConfig,
) -> KeyboardConfig {
    KeyboardConfig {
        history_previous: user
            .as_ref()
            .and_then(|k| k.history_previous.clone())
            .or_else(|| default.history_previous.clone()),
        history_next: user
            .as_ref()
            .and_then(|k| k.history_next.clone())
            .or_else(|| default.history_next.clone()),
        enter_directory: user
            .as_ref()
            .and_then(|k| k.enter_directory.clone())
            .or_else(|| default.enter_directory.clone()),
        directory_up: user
            .as_ref()
            .and_then(|k| k.directory_up.clone())
            .or_else(|| default.directory_up.clone()),
        edit: user
            .as_ref()
            .and_then(|k| k.edit.clone())
            .or_else(|| default.edit.clone()),
        new_tab: user
            .as_ref()
            .and_then(|k| k.new_tab.clone())
            .or_else(|| default.new_tab.clone()),
        next_tab: user
            .as_ref()
            .and_then(|k| k.next_tab.clone())
            .or_else(|| default.next_tab.clone()),
        prev_tab: user
            .as_ref()
            .and_then(|k| k.prev_tab.clone())
            .or_else(|| default.prev_tab.clone()),
        close_tab: user
            .as_ref()
            .and_then(|k| k.close_tab.clone())
            .or_else(|| default.close_tab.clone()),
        fuzzy_search: user
            .as_ref()
            .and_then(|k| k.fuzzy_search.clone())
            .or_else(|| default.fuzzy_search.clone()),
        sort_by_name: user
            .as_ref()
            .and_then(|k| k.sort_by_name.clone())
            .or_else(|| default.sort_by_name.clone()),
        sort_by_extension: user
            .as_ref()
            .and_then(|k| k.sort_by_extension.clone())
            .or_else(|| default.sort_by_extension.clone()),
        sort_by_date: user
            .as_ref()
            .and_then(|k| k.sort_by_date.clone())
            .or_else(|| default.sort_by_date.clone()),
        sort_by_size: user
            .as_ref()
            .and_then(|k| k.sort_by_size.clone())
            .or_else(|| default.sort_by_size.clone()),
        rename: user
            .as_ref()
            .and_then(|k| k.rename.clone())
            .or_else(|| default.rename.clone()),
        delete: user
            .as_ref()
            .and_then(|k| k.delete.clone())
            .or_else(|| default.delete.clone()),
        delete_permanently: user
            .as_ref()
            .and_then(|k| k.delete_permanently.clone())
            .or_else(|| default.delete_permanently.clone()),
        task_manager: user
            .as_ref()
            .and_then(|k| k.task_manager.clone())
            .or_else(|| default.task_manager.clone()),
        select_all: user
            .as_ref()
            .and_then(|k| k.select_all.clone())
            .or_else(|| default.select_all.clone()),
    }
}

pub fn merge_theme_config(
    user: &Option<ThemeConfig>,
    default: &ThemeConfig,
) -> Result<ThemeConfig, String> {
    let name = user
        .as_ref()
        .and_then(|t| t.name.clone())
        .or_else(|| default.name.clone());

    // Validate theme name
    if let Some(ref n) = name
        && !crate::theme::THEME_NAMES.contains(&n.as_str())
    {
        return Err(format!(
            "Invalid theme '{}'. Available themes: {:?}",
            n,
            crate::theme::THEME_NAMES
        ));
    }

    Ok(ThemeConfig {
        name,
        // Add more theme fields as needed
    })
}

pub fn load_config() -> Result<(KeyboardConfig, ThemeConfig), String> {
    let path = config_path().ok_or("Could not determine config directory")?;
    let default_keyboard = default_keyboard_config();
    let default_theme = default_theme_config();
    if path.exists() {
        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read config file: {}", e))?;
        let user_config: AppConfig =
            toml::from_str(&content).map_err(|e| format!("Config file is invalid: {}", e))?;
        let keyboard = merge_keyboard_config(&user_config.keyboard, &default_keyboard);
        let theme = merge_theme_config(&user_config.theme, &default_theme)?;
        Ok((keyboard, theme))
    } else {
        Ok((default_keyboard, default_theme))
    }
}

pub fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "fm", "fm").map(|proj_dirs| proj_dirs.config_dir().join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_theme_config_invalid() {
        let default = default_theme_config();
        let user = Some(ThemeConfig {
            name: Some("invalid_theme_name".to_string()),
        });

        let result = merge_theme_config(&user, &default);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid theme"));
    }
}
