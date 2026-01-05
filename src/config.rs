use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct KeyboardConfig {
    pub new_file: Option<Vec<String>>,
    pub quit: Option<Vec<String>>,
    pub back: Option<Vec<String>>,
    pub forward: Option<Vec<String>>,
    pub enter_dir: Option<Vec<String>>,
    pub up_dir: Option<Vec<String>>,
    pub edit_file: Option<Vec<String>>,
    pub new_tab: Option<Vec<String>>,
    pub tab_next: Option<Vec<String>>,
    pub tab_prev: Option<Vec<String>>,
    pub tab_close: Option<Vec<String>>,
    pub search: Option<Vec<String>>,
    pub sort_name: Option<Vec<String>>,
    pub sort_ext: Option<Vec<String>>,
    pub sort_date: Option<Vec<String>>,
    pub sort_size: Option<Vec<String>>,
    pub copy_to: Option<Vec<String>>,
    pub move_to: Option<Vec<String>>,
    pub rename: Option<Vec<String>>,
    pub delete: Option<Vec<String>>,
    pub delete_force: Option<Vec<String>>,
    pub empty_trash: Option<Vec<String>>,
    pub tasks: Option<Vec<String>>,
    pub select_all: Option<Vec<String>>,
    pub new_dir: Option<Vec<String>>,
    pub open_terminal: Option<Vec<String>>,
    pub help: Option<Vec<String>>,
    pub change_drive_left: Option<Vec<String>>,
    pub change_drive_right: Option<Vec<String>>,
    pub toggle_console: Option<Vec<String>>,
    pub swap_tabs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct EditorConfig {
    pub command: Option<String>,
    pub in_terminal: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ViewerConfig {
    pub command: Option<String>,
    pub in_terminal: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct GlobalConfig {
    pub theme: Option<String>,
    pub terminal: Option<String>,
    pub editor: Option<String>, // deprecated: string fallback, prefer [editor]
    pub viewer: Option<String>, // deprecated: string fallback, prefer [viewer]
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct AppConfig {
    pub global: Option<GlobalConfig>,
    pub keyboard: Option<KeyboardConfig>,
    pub editor: Option<EditorConfig>,
    pub viewer: Option<ViewerConfig>,
}

// Default key bindings
pub fn default_keyboard_config() -> KeyboardConfig {
    KeyboardConfig {
        new_file: Some(vec!["Shift-F4".to_string()]),
        quit: Some(vec!["Ctrl-q".to_string()]),
        forward: Some(vec!["Shift-Right".to_string()]),
        back: Some(vec!["Shift-Left".to_string()]),
        enter_dir: Some(vec!["Right".to_string()]),
        up_dir: Some(vec!["Backspace".to_string(), "Left".to_string()]),
        edit_file: Some(vec!["F4".to_string()]),
        new_tab: Some(vec!["Ctrl-t".to_string()]),
        tab_next: Some(vec!["Alt-Right".to_string()]),
        tab_prev: Some(vec!["Alt-Left".to_string()]),
        tab_close: Some(vec!["Ctrl-w".to_string()]),
        search: Some(vec!["Ctrl-p".to_string()]),
        sort_name: Some(vec!["Ctrl-F2".to_string()]),
        sort_ext: Some(vec!["Ctrl-F4".to_string()]),
        sort_date: Some(vec!["Ctrl-F5".to_string()]),
        sort_size: Some(vec!["Ctrl-F6".to_string()]),
        copy_to: Some(vec!["F5".to_string()]),
        move_to: Some(vec!["F6".to_string()]),
        rename: Some(vec!["F2".to_string()]),
        delete: Some(vec!["Delete".to_string()]),
        delete_force: Some(vec!["Shift-Delete".to_string()]),
        empty_trash: Some(vec!["Ctrl-F8".to_string()]),
        tasks: Some(vec!["F10".to_string()]),
        new_dir: Some(vec!["F7".to_string()]),
        select_all: Some(vec!["Ctrl-a".to_string()]),
        open_terminal: Some(vec!["F9".to_string()]),
        help: Some(vec!["F1".to_string()]),
        change_drive_left: Some(vec!["Alt-F1".to_string()]),
        change_drive_right: Some(vec!["Alt-F2".to_string()]),
        toggle_console: Some(vec!["Ctrl-o".to_string()]),
        swap_tabs: Some(vec!["Ctrl-u".to_string()]),
    }
}

pub fn default_global_config() -> GlobalConfig {
    GlobalConfig {
        theme: Some("mariana".to_string()),
        terminal: None,
        editor: None,
        viewer: None,
    }
}

pub fn merge_keyboard_config(
    user: Option<&KeyboardConfig>,
    default: &KeyboardConfig,
) -> KeyboardConfig {
    KeyboardConfig {
        new_file: user
            .and_then(|k| k.new_file.clone())
            .or_else(|| default.new_file.clone()),
        quit: user
            .and_then(|k| k.quit.clone())
            .or_else(|| default.quit.clone()),
        forward: user
            .and_then(|k| k.forward.clone())
            .or_else(|| default.forward.clone()),
        back: user
            .and_then(|k| k.back.clone())
            .or_else(|| default.back.clone()),
        enter_dir: user
            .and_then(|k| k.enter_dir.clone())
            .or_else(|| default.enter_dir.clone()),
        up_dir: user
            .and_then(|k| k.up_dir.clone())
            .or_else(|| default.up_dir.clone()),
        edit_file: user
            .and_then(|k| k.edit_file.clone())
            .or_else(|| default.edit_file.clone()),
        new_tab: user
            .and_then(|k| k.new_tab.clone())
            .or_else(|| default.new_tab.clone()),
        tab_next: user
            .and_then(|k| k.tab_next.clone())
            .or_else(|| default.tab_next.clone()),
        tab_prev: user
            .and_then(|k| k.tab_prev.clone())
            .or_else(|| default.tab_prev.clone()),
        tab_close: user
            .and_then(|k| k.tab_close.clone())
            .or_else(|| default.tab_close.clone()),
        search: user
            .and_then(|k| k.search.clone())
            .or_else(|| default.search.clone()),
        sort_name: user
            .and_then(|k| k.sort_name.clone())
            .or_else(|| default.sort_name.clone()),
        sort_ext: user
            .and_then(|k| k.sort_ext.clone())
            .or_else(|| default.sort_ext.clone()),
        sort_date: user
            .and_then(|k| k.sort_date.clone())
            .or_else(|| default.sort_date.clone()),
        sort_size: user
            .and_then(|k| k.sort_size.clone())
            .or_else(|| default.sort_size.clone()),
        copy_to: user
            .and_then(|k| k.copy_to.clone())
            .or_else(|| default.copy_to.clone()),
        move_to: user
            .and_then(|k| k.move_to.clone())
            .or_else(|| default.move_to.clone()),
        rename: user
            .and_then(|k| k.rename.clone())
            .or_else(|| default.rename.clone()),
        delete: user
            .and_then(|k| k.delete.clone())
            .or_else(|| default.delete.clone()),
        delete_force: user
            .and_then(|k| k.delete_force.clone())
            .or_else(|| default.delete_force.clone()),
        empty_trash: user
            .and_then(|k| k.empty_trash.clone())
            .or_else(|| default.empty_trash.clone()),
        tasks: user
            .and_then(|k| k.tasks.clone())
            .or_else(|| default.tasks.clone()),
        select_all: user
            .and_then(|k| k.select_all.clone())
            .or_else(|| default.select_all.clone()),
        new_dir: user
            .and_then(|k| k.new_dir.clone())
            .or_else(|| default.new_dir.clone()),
        open_terminal: user
            .and_then(|k| k.open_terminal.clone())
            .or_else(|| default.open_terminal.clone()),
        help: user
            .and_then(|k| k.help.clone())
            .or_else(|| default.help.clone()),
        change_drive_left: user
            .and_then(|k| k.change_drive_left.clone())
            .or_else(|| default.change_drive_left.clone()),
        change_drive_right: user
            .and_then(|k| k.change_drive_right.clone())
            .or_else(|| default.change_drive_right.clone()),
        toggle_console: user
            .and_then(|k| k.toggle_console.clone())
            .or_else(|| default.toggle_console.clone()),
        swap_tabs: user
            .and_then(|k| k.swap_tabs.clone())
            .or_else(|| default.swap_tabs.clone()),
    }
}

pub fn merge_global_config(
    user: Option<&GlobalConfig>,
    default: &GlobalConfig,
) -> Result<GlobalConfig, String> {
    let theme = user
        .and_then(|g| g.theme.clone())
        .or_else(|| default.theme.clone());
    let terminal = user
        .and_then(|g| g.terminal.clone())
        .or_else(|| default.terminal.clone());

    let editor = user
        .and_then(|g| g.editor.clone())
        .or_else(|| default.editor.clone());
    let viewer = user
        .and_then(|g| g.viewer.clone())
        .or_else(|| default.viewer.clone());

    // Validate theme name
    if let Some(ref n) = theme
        && !crate::theme::THEME_NAMES.contains(&n.as_str())
    {
        return Err(format!(
            "Invalid theme '{}'. Available themes: {:?}",
            n,
            crate::theme::THEME_NAMES
        ));
    }

    Ok(GlobalConfig {
        theme,
        terminal,
        editor,
        viewer,
    })
}

pub fn validate_keyboard_config(config: &KeyboardConfig) -> Result<(), String> {
    use std::collections::HashMap;
    let mut keys_to_actions: HashMap<String, String> = HashMap::new();

    let fields = [
        ("new_file", &config.new_file),
        ("quit", &config.quit),
        ("forward", &config.forward),
        ("back", &config.back),
        ("enter_dir", &config.enter_dir),
        ("up_dir", &config.up_dir),
        ("edit_file", &config.edit_file),
        ("new_tab", &config.new_tab),
        ("tab_next", &config.tab_next),
        ("tab_prev", &config.tab_prev),
        ("tab_close", &config.tab_close),
        ("search", &config.search),
        ("sort_name", &config.sort_name),
        ("sort_ext", &config.sort_ext),
        ("sort_date", &config.sort_date),
        ("sort_size", &config.sort_size),
        ("copy_to", &config.copy_to),
        ("move_to", &config.move_to),
        ("rename", &config.rename),
        ("delete", &config.delete),
        ("delete_force", &config.delete_force),
        ("empty_trash", &config.empty_trash),
        ("tasks", &config.tasks),
        ("select_all", &config.select_all),
        ("new_dir", &config.new_dir),
        ("open_terminal", &config.open_terminal),
        ("help", &config.help),
        ("change_drive_left", &config.change_drive_left),
        ("change_drive_right", &config.change_drive_right),
        ("toggle_console", &config.toggle_console),
        ("swap_tabs", &config.swap_tabs),
    ];

    for (name, keys) in fields {
        if let Some(keys) = keys {
            for key in keys {
                if let Some(other_action) = keys_to_actions.insert(key.clone(), name.to_string())
                    && other_action != name
                {
                    return Err(format!(
                        "Keybinding conflict: '{key}' is assigned to both '{other_action}' and '{name}'"
                    ));
                }
            }
        }
    }

    Ok(())
}

pub(crate) fn split_command(cmd: &str) -> Vec<String> {
    let trimmed = cmd.trim();
    if std::path::Path::new(trimmed).exists() {
        return vec![trimmed.to_string()];
    }

    #[cfg(target_os = "windows")]
    {
        let alt = trimmed.replace('/', "\\");
        if std::path::Path::new(&alt).exists() {
            return vec![alt];
        }
        let alt2 = trimmed.replace('\\', "/");
        if std::path::Path::new(&alt2).exists() {
            return vec![alt2];
        }
    }

    if let Ok(parts) = shell_words::split(cmd) {
        parts
    } else {
        // Fallback for cases where shell_words fails, e.g. unclosed quotes
        // Or simple whitespace split if it's not a shell-like command
        cmd.split_whitespace().map(|s| s.to_string()).collect()
    }
}

pub fn validate_editor_config(config: &EditorConfig) -> Result<(), String> {
    if let Some(cmd) = &config.command {
        if cmd.trim().is_empty() {
            return Err("Editor command cannot be empty".to_string());
        }
        let parts = split_command(cmd);
        if let Some(first_part) = parts.first() {
            let path = std::path::Path::new(first_part);
            if path.is_absolute() && !path.exists() {
                // On Windows, also try with forward/backward slashes swapped if it's absolute
                #[cfg(target_os = "windows")]
                {
                    let alt_path = first_part.replace('/', "\\");
                    let alt_path2 = first_part.replace('\\', "/");
                    if !std::path::Path::new(&alt_path).exists()
                        && !std::path::Path::new(&alt_path2).exists()
                    {
                        return Err(format!("Editor command path does not exist: {first_part}"));
                    }
                }
                #[cfg(not(target_os = "windows"))]
                return Err(format!("Editor command path does not exist: {first_part}"));
            }
        }
    }
    Ok(())
}

pub fn validate_viewer_config(config: &ViewerConfig) -> Result<(), String> {
    if let Some(cmd) = &config.command {
        if cmd.trim().is_empty() {
            return Err("Viewer command cannot be empty".to_string());
        }
        let parts = split_command(cmd);
        if let Some(first_part) = parts.first() {
            let path = std::path::Path::new(first_part);
            if path.is_absolute() && !path.exists() {
                #[cfg(target_os = "windows")]
                {
                    let alt_path = first_part.replace('/', "\\");
                    let alt_path2 = first_part.replace('\\', "/");
                    if !std::path::Path::new(&alt_path).exists()
                        && !std::path::Path::new(&alt_path2).exists()
                    {
                        return Err(format!("Viewer command path does not exist: {first_part}"));
                    }
                }
                #[cfg(not(target_os = "windows"))]
                return Err(format!("Viewer command path does not exist: {first_part}"));
            }
        }
    }
    Ok(())
}

pub fn validate_global_config(config: &GlobalConfig) -> Result<(), String> {
    let check_cmd = |cmd: &Option<String>, name: &str| -> Result<(), String> {
        if let Some(c) = cmd {
            if c.trim().is_empty() {
                return Err(format!("Global {name} command cannot be empty"));
            }
            let parts = split_command(c);
            if let Some(first_part) = parts.first() {
                let path = std::path::Path::new(first_part);
                if path.is_absolute() && !path.exists() {
                    #[cfg(target_os = "windows")]
                    {
                        let alt_path = first_part.replace('/', "\\");
                        let alt_path2 = first_part.replace('\\', "/");
                        if !std::path::Path::new(&alt_path).exists()
                            && !std::path::Path::new(&alt_path2).exists()
                        {
                            return Err(format!(
                                "Global {name} command path does not exist: {first_part}"
                            ));
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    return Err(format!(
                        "Global {name} command path does not exist: {first_part}"
                    ));
                }
            }
        }
        Ok(())
    };

    check_cmd(&config.terminal, "terminal")?;
    check_cmd(&config.editor, "editor")?;
    check_cmd(&config.viewer, "viewer")?;
    Ok(())
}

pub fn load_config() -> Result<(KeyboardConfig, GlobalConfig, EditorConfig, ViewerConfig), String> {
    let path = config_path().ok_or("Could not determine config directory")?;
    let default_keyboard = default_keyboard_config();
    let default_global = default_global_config();
    let default_editor = EditorConfig {
        command: None,
        in_terminal: Some(true),
    };
    let default_viewer = ViewerConfig {
        command: None,
        in_terminal: Some(true),
    };
    let (keyboard, global, editor, viewer) = if path.exists() {
        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read config file: {e}"))?;
        let user_config: AppConfig =
            toml::from_str(&content).map_err(|e| format!("Config file is invalid: {e}"))?;
        let keyboard = merge_keyboard_config(user_config.keyboard.as_ref(), &default_keyboard);
        let global = merge_global_config(user_config.global.as_ref(), &default_global)?;
        let editor = user_config.editor.unwrap_or(default_editor);
        let viewer = user_config.viewer.unwrap_or(default_viewer);
        (keyboard, global, editor, viewer)
    } else {
        (
            default_keyboard,
            default_global,
            default_editor,
            default_viewer,
        )
    };

    validate_keyboard_config(&keyboard)?;
    validate_global_config(&global)?;
    validate_editor_config(&editor)?;
    validate_viewer_config(&viewer)?;

    Ok((keyboard, global, editor, viewer))
}

pub fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "fm", "fm").map(|proj_dirs| proj_dirs.config_dir().join("config.toml"))
}
pub fn create_default_config() -> Result<PathBuf, String> {
    let path = config_path().ok_or("Could not determine config directory")?;
    if path.exists() {
        return Err(format!("Config file already exists at {}", path.display()));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {e}"))?;
    }

    let default_config = AppConfig {
        global: Some(default_global_config()),
        keyboard: Some(default_keyboard_config()),
        editor: Some(EditorConfig::default()),
        viewer: Some(ViewerConfig::default()),
    };

    let toml_content = toml::to_string_pretty(&default_config)
        .map_err(|e| format!("Failed to serialize default config: {e}"))?;

    fs::write(&path, toml_content).map_err(|e| format!("Failed to write config file: {e}"))?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path_returns_some() {
        let path = config_path();
        assert!(path.is_some(), "Config path should return Some(PathBuf)");
    }

    #[test]
    fn test_merge_keyboard_config_prefers_user() {
        let user = Some(KeyboardConfig {
            new_file: Some(vec!["user-key".to_string()]),
            ..KeyboardConfig::default()
        });
        let default = KeyboardConfig {
            new_file: Some(vec!["default-key".to_string()]),
            ..KeyboardConfig::default()
        };
        let merged = merge_keyboard_config(user.as_ref(), &default);
        assert_eq!(merged.new_file, Some(vec!["user-key".to_string()]));
    }

    #[test]
    fn test_merge_keyboard_config_fallbacks_to_default() {
        let user = None;
        let default = KeyboardConfig {
            new_file: Some(vec!["default-key".to_string()]),
            ..KeyboardConfig::default()
        };
        let merged = merge_keyboard_config(user, &default);
        assert_eq!(merged.new_file, Some(vec!["default-key".to_string()]));
    }

    #[test]
    fn test_merge_global_config_theme_validation() {
        let user = Some(GlobalConfig {
            theme: Some("invalid".to_string()),
            ..GlobalConfig::default()
        });
        let default = GlobalConfig {
            theme: Some("mariana".to_string()),
            ..GlobalConfig::default()
        };
        let result = merge_global_config(user.as_ref(), &default);
        assert!(result.is_err(), "Invalid theme should error");
    }

    #[test]
    fn test_merge_global_config_valid_theme() {
        let valid_theme = crate::theme::THEME_NAMES.iter().next().unwrap().to_string();
        let user = Some(GlobalConfig {
            theme: Some(valid_theme.clone()),
            ..GlobalConfig::default()
        });
        let default = GlobalConfig::default();
        let result = merge_global_config(user.as_ref(), &default);
        assert!(result.is_ok(), "Valid theme should not error");
        assert_eq!(result.unwrap().theme, Some(valid_theme));
    }

    #[test]
    fn test_default_keyboard_and_global_config() {
        let keyboard = default_keyboard_config();
        let global = default_global_config();
        assert!(keyboard.quit.is_some());
        assert!(global.theme.is_some());
    }

    #[test]
    fn test_validate_keyboard_config_no_conflict() {
        let config = default_keyboard_config();
        assert!(validate_keyboard_config(&config).is_ok());
    }

    #[test]
    fn test_validate_keyboard_config_conflict() {
        let mut config = default_keyboard_config();
        // Conflict: assign 'q' to both quit and new_file
        config.quit = Some(vec!["q".to_string()]);
        config.new_file = Some(vec!["q".to_string()]);
        let result = validate_keyboard_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Keybinding conflict"));
    }

    #[test]
    fn test_validate_keyboard_config_same_action_no_conflict() {
        let mut config = default_keyboard_config();
        // Same key twice for the same action is NOT a conflict (though redundant)
        config.quit = Some(vec!["q".to_string(), "q".to_string()]);
        assert!(validate_keyboard_config(&config).is_ok());
    }

    #[test]
    fn test_validate_editor_viewer_config() {
        let editor = EditorConfig {
            command: Some("vim".to_string()),
            in_terminal: Some(true),
        };
        assert!(validate_editor_config(&editor).is_ok());

        let editor_empty = EditorConfig {
            command: Some("".to_string()),
            in_terminal: Some(true),
        };
        assert!(validate_editor_config(&editor_empty).is_err());

        let editor_whitespace = EditorConfig {
            command: Some("  ".to_string()),
            in_terminal: Some(true),
        };
        assert!(validate_editor_config(&editor_whitespace).is_err());

        let viewer = ViewerConfig {
            command: Some("less".to_string()),
            in_terminal: Some(true),
        };
        assert!(validate_viewer_config(&viewer).is_ok());

        // Absolute path that does NOT exist
        let editor_abs_no_exist = EditorConfig {
            command: Some("/non/existent/path/to/editor".to_string()),
            in_terminal: Some(true),
        };
        assert!(validate_editor_config(&editor_abs_no_exist).is_err());

        // Absolute path that DOES exist
        // /bin/sh or /bin/ls should exist on most Unix systems
        let cmd = if cfg!(windows) {
            "C:\\Windows\\System32\\cmd.exe"
        } else {
            "/bin/sh"
        };
        let editor_abs_exist = EditorConfig {
            command: Some(cmd.to_string()),
            in_terminal: Some(true),
        };
        if std::path::Path::new(cmd).exists() {
            assert!(validate_editor_config(&editor_abs_exist).is_ok());
        }
    }

    #[test]
    fn test_split_command() {
        // Test simple command
        assert_eq!(split_command("vim"), vec!["vim".to_string()]);
        // Test command with args
        assert_eq!(
            split_command("vim -u NONE"),
            vec!["vim".to_string(), "-u".to_string(), "NONE".to_string()]
        );
        // Test quoted path with spaces
        assert_eq!(
            split_command("\"C:/Program Files/vim.exe\" -v"),
            vec!["C:/Program Files/vim.exe".to_string(), "-v".to_string()]
        );
        // Test unquoted path with spaces (will split if not exists, which is true on Linux test env)
        let parts = split_command("C:/Program Files/vim.exe");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            parts,
            vec!["C:/Program".to_string(), "Files/vim.exe".to_string()]
        );
    }
}
