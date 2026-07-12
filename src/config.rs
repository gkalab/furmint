use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct KeyboardConfig {
    pub new_file: Option<Vec<String>>,
    pub quit: Option<Vec<String>>,
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
    pub open_ssh: Option<Vec<String>>,
    pub reconnect_ssh: Option<Vec<String>>,
    pub calc_dir_size: Option<Vec<String>>,
    pub rename_tab: Option<Vec<String>>,
    pub add_bookmark: Option<Vec<String>>,
    pub open_bookmarks: Option<Vec<String>>,
    pub viewer_search: Option<Vec<String>>,
    pub viewer_search_next: Option<Vec<String>>,
    pub viewer_search_prev: Option<Vec<String>>,
    pub tab_move_left: Option<Vec<String>>,
    pub tab_move_right: Option<Vec<String>>,
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
pub struct SshConfig {
    pub keepalive_interval: Option<u32>,
    pub read_timeout_secs: Option<u64>,
    pub watchdog_secs: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct GlobalConfig {
    pub theme: Option<String>,
    pub terminal: Option<String>,
    pub borders: Option<bool>,
    pub icons: Option<bool>,
    pub mouse: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct AppConfig {
    pub global: Option<GlobalConfig>,
    pub keyboard: Option<KeyboardConfig>,
    pub editor: Option<EditorConfig>,
    pub viewer: Option<ViewerConfig>,
    pub ssh: Option<SshConfig>,
}

// Default key bindings
#[must_use]
pub fn default_keyboard_config() -> KeyboardConfig {
    KeyboardConfig {
        new_file: Some(vec!["Shift-F4".to_string()]),
        quit: Some(vec!["Ctrl-q".to_string()]),
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
        open_ssh: Some(vec!["Ctrl-n".to_string()]),
        reconnect_ssh: Some(vec!["Ctrl-F11".to_string()]),
        calc_dir_size: Some(vec!["Ctrl-Space".to_string()]),
        rename_tab: Some(vec!["Ctrl-r".to_string()]),
        add_bookmark: Some(vec!["Ctrl-d".to_string()]),
        open_bookmarks: Some(vec!["Ctrl-b".to_string()]),
        viewer_search: Some(vec!["Ctrl-f".to_string(), "/".to_string()]),
        viewer_search_next: Some(vec!["n".to_string(), "F3".to_string()]),
        viewer_search_prev: Some(vec!["N".to_string(), "Shift-F3".to_string()]),
        tab_move_left: Some(vec!["Ctrl-Shift-Left".to_string()]),
        tab_move_right: Some(vec!["Ctrl-Shift-Right".to_string()]),
    }
}

#[must_use]
pub fn default_global_config() -> GlobalConfig {
    GlobalConfig {
        theme: Some("mariana".to_string()),
        terminal: None,
        borders: Some(true),
        icons: Some(false),
        mouse: Some(true),
    }
}

#[must_use]
pub fn merge_keyboard_config(
    user: Option<&KeyboardConfig>,
    default: &KeyboardConfig,
) -> KeyboardConfig {
    macro_rules! merge_opt {
        ($field:ident) => {
            user.and_then(|k| k.$field.clone())
                .or_else(|| default.$field.clone())
        };
    }

    KeyboardConfig {
        new_file: merge_opt!(new_file),
        quit: merge_opt!(quit),
        enter_dir: merge_opt!(enter_dir),
        up_dir: merge_opt!(up_dir),
        edit_file: merge_opt!(edit_file),
        new_tab: merge_opt!(new_tab),
        tab_next: merge_opt!(tab_next),
        tab_prev: merge_opt!(tab_prev),
        tab_close: merge_opt!(tab_close),
        search: merge_opt!(search),
        sort_name: merge_opt!(sort_name),
        sort_ext: merge_opt!(sort_ext),
        sort_date: merge_opt!(sort_date),
        sort_size: merge_opt!(sort_size),
        copy_to: merge_opt!(copy_to),
        move_to: merge_opt!(move_to),
        rename: merge_opt!(rename),
        delete: merge_opt!(delete),
        delete_force: merge_opt!(delete_force),
        empty_trash: merge_opt!(empty_trash),
        tasks: merge_opt!(tasks),
        select_all: merge_opt!(select_all),
        new_dir: merge_opt!(new_dir),
        open_terminal: merge_opt!(open_terminal),
        help: merge_opt!(help),
        change_drive_left: merge_opt!(change_drive_left),
        change_drive_right: merge_opt!(change_drive_right),
        toggle_console: merge_opt!(toggle_console),
        swap_tabs: merge_opt!(swap_tabs),
        open_ssh: merge_opt!(open_ssh),
        reconnect_ssh: merge_opt!(reconnect_ssh),
        calc_dir_size: merge_opt!(calc_dir_size),
        rename_tab: merge_opt!(rename_tab),
        add_bookmark: merge_opt!(add_bookmark),
        open_bookmarks: merge_opt!(open_bookmarks),
        viewer_search: merge_opt!(viewer_search),
        viewer_search_next: merge_opt!(viewer_search_next),
        viewer_search_prev: merge_opt!(viewer_search_prev),
        tab_move_left: merge_opt!(tab_move_left),
        tab_move_right: merge_opt!(tab_move_right),
    }
}

/// Merges user config with default config, preferring user values when present.
///
/// # Errors
///
/// Returns an error if the theme validation fails.
pub fn merge_global_config(
    user: Option<&GlobalConfig>,
    default: &GlobalConfig,
) -> Result<GlobalConfig> {
    let theme = user
        .and_then(|g| g.theme.clone())
        .or_else(|| default.theme.clone());
    let terminal = user
        .and_then(|g| g.terminal.clone())
        .or_else(|| default.terminal.clone());
    let borders = user.and_then(|g| g.borders).or(default.borders);
    let icons = user.and_then(|g| g.icons).or(default.icons);
    let mouse = user.and_then(|g| g.mouse).or(default.mouse);

    // Validate theme name
    if let Some(ref n) = theme
        && !crate::theme::THEME_NAMES.contains(&n.as_str())
    {
        return Err(anyhow!(
            "Invalid theme '{}'. Available themes: {:?}",
            n,
            crate::theme::THEME_NAMES
        ));
    }

    Ok(GlobalConfig {
        theme,
        terminal,
        borders,
        icons,
        mouse,
    })
}

/// Validates keyboard configuration for duplicate bindings.
///
/// # Errors
///
/// Returns an error if duplicate key bindings are found.
pub fn validate_keyboard_config(config: &KeyboardConfig) -> Result<()> {
    use std::collections::HashMap;
    let mut keys_to_actions: HashMap<String, String> = HashMap::new();

    let fields = [
        ("new_file", &config.new_file),
        ("quit", &config.quit),
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
        ("open_ssh", &config.open_ssh),
        ("reconnect_ssh", &config.reconnect_ssh),
        ("calc_dir_size", &config.calc_dir_size),
        ("rename_tab", &config.rename_tab),
        ("add_bookmark", &config.add_bookmark),
        ("open_bookmarks", &config.open_bookmarks),
        ("viewer_search", &config.viewer_search),
        ("viewer_search_next", &config.viewer_search_next),
        ("viewer_search_prev", &config.viewer_search_prev),
        ("tab_move_left", &config.tab_move_left),
        ("tab_move_right", &config.tab_move_right),
    ];

    for (name, keys) in fields {
        if let Some(keys) = keys {
            for key in keys {
                if let Some(other_action) = keys_to_actions.insert(key.clone(), name.to_string())
                    && other_action != name
                {
                    return Err(anyhow!(
                        "Keybinding conflict: '{key}' is assigned to both '{other_action}' and '{name}'"
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Helper to check if a program exists, trying with common extensions if needed.
fn check_program_exists(p: &str) -> Option<String> {
    let path = std::path::Path::new(p);
    if path.exists() {
        return Some(p.to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let p_win = p.replace('/', "\\");
        if std::path::Path::new(&p_win).exists() {
            return Some(p_win);
        }
        for ext in [".exe", ".cmd", ".bat", ".com"] {
            let with_ext = format!("{p}{ext}");
            if std::path::Path::new(&with_ext).exists() {
                return Some(with_ext);
            }
            let with_ext_win = with_ext.replace('/', "\\");
            if std::path::Path::new(&with_ext_win).exists() {
                return Some(with_ext_win);
            }
        }
    }
    None
}

/// Parses a command string into (program, args).
/// Handles Windows paths with spaces by trying progressively longer prefixes.
#[must_use]
pub fn parse_command(cmd: &str) -> (String, Vec<String>) {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return (String::new(), vec![]);
    }

    // Windows-specific path detection
    let is_windows_abs = trimmed.len() > 2
        && trimmed.get(1..2) == Some(":")
        && trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic());

    if is_windows_abs && !trimmed.starts_with('"') && !trimmed.contains('\'') {
        let parts: Vec<String> = trimmed
            .split_whitespace()
            .map(std::string::ToString::to_string)
            .collect();

        // Try progressively longer prefixes (from longest to shortest)
        for i in (0..parts.len()).rev() {
            let candidate = parts[..=i].join(" ");
            if let Some(existing) = check_program_exists(&candidate) {
                let args = parts[i + 1..].to_vec();
                return (existing, args);
            }
        }

        // Fallback for unquoted absolute paths that aren't found on disk:
        // if no clear argument markers are present, treat it as one string.
        if !trimmed.contains(" -") && !trimmed.contains(" /") {
            return (trimmed.to_string(), vec![]);
        }
    }

    // Standard shell splitting
    if let Ok(parts) = shell_words::split(trimmed) {
        if parts.is_empty() {
            (String::new(), vec![])
        } else {
            (parts[0].clone(), parts[1..].to_vec())
        }
    } else {
        let parts: Vec<String> = trimmed
            .split_whitespace()
            .map(std::string::ToString::to_string)
            .collect();
        if parts.is_empty() {
            (String::new(), vec![])
        } else {
            (parts[0].clone(), parts[1..].to_vec())
        }
    }
}

/// Validates editor configuration.
///
/// # Errors
///
/// Returns an error if the editor command is invalid.
pub fn validate_editor_config(config: &EditorConfig) -> Result<()> {
    if let Some(cmd) = &config.command {
        if cmd.trim().is_empty() {
            return Err(anyhow!("Editor command cannot be empty"));
        }
        let (program, _) = parse_command(cmd);
        let path = std::path::Path::new(&program);
        if path.is_absolute() && check_program_exists(&program).is_none() {
            return Err(anyhow!("Editor command path does not exist: {program}"));
        }
    }
    Ok(())
}

/// Validates viewer configuration.
///
/// # Errors
///
/// Returns an error if the viewer command is invalid.
pub fn validate_viewer_config(config: &ViewerConfig) -> Result<()> {
    if let Some(cmd) = &config.command {
        if cmd.trim().is_empty() {
            return Err(anyhow!("Viewer command cannot be empty"));
        }
        let (program, _) = parse_command(cmd);
        let path = std::path::Path::new(&program);
        if path.is_absolute() && check_program_exists(&program).is_none() {
            return Err(anyhow!("Viewer command path does not exist: {program}"));
        }
    }
    Ok(())
}

/// Validates SSH configuration.
///
/// # Errors
///
/// Returns an error if the SSH configuration is invalid.
pub fn validate_ssh_config(config: &SshConfig) -> Result<()> {
    if let Some(keepalive) = config.keepalive_interval {
        if keepalive == 0 {
            return Err(anyhow!("SSH keepalive_interval must be greater than 0"));
        }
        if keepalive > 3600 {
            return Err(anyhow!(
                "SSH keepalive_interval must be less than or equal to 3600 seconds (1 hour)"
            ));
        }
    }

    if let Some(timeout) = config.read_timeout_secs {
        if timeout == 0 {
            return Err(anyhow!("SSH read_timeout_secs must be greater than 0"));
        }
        if timeout > 3600 {
            return Err(anyhow!(
                "SSH read_timeout_secs must be less than or equal to 3600 seconds (1 hour)"
            ));
        }
    }

    if let Some(watchdog) = config.watchdog_secs {
        if watchdog == 0 {
            return Err(anyhow!("SSH watchdog_secs must be greater than 0"));
        }
        if watchdog > 3600 {
            return Err(anyhow!(
                "SSH watchdog_secs must be less than or equal to 3600 seconds (1 hour)"
            ));
        }
    }

    if let (Some(timeout), Some(watchdog)) = (config.read_timeout_secs, config.watchdog_secs)
        && timeout > watchdog
    {
        return Err(anyhow!(
            "SSH read_timeout_secs ({timeout}) must be less than or equal to watchdog_secs ({watchdog})"
        ));
    }

    Ok(())
}

/// Validates global configuration.
///
/// # Errors
///
/// Returns an error if any configuration value is invalid.
pub fn validate_global_config(config: &GlobalConfig) -> Result<()> {
    let check_cmd = |cmd: &Option<String>, name: &str| -> Result<()> {
        if let Some(c) = cmd {
            if c.trim().is_empty() {
                return Err(anyhow!("Global {name} command cannot be empty"));
            }
            let (program, _) = parse_command(c);
            let path = std::path::Path::new(&program);
            if path.is_absolute() && check_program_exists(&program).is_none() {
                return Err(anyhow!(
                    "Global {name} command path does not exist: {program}"
                ));
            }
        }
        Ok(())
    };

    check_cmd(&config.terminal, "terminal")?;
    Ok(())
}

/// Loads the application configuration.
///
/// # Errors
///
/// Returns an error if the configuration cannot be loaded or is invalid.
pub fn load_config() -> Result<(
    KeyboardConfig,
    GlobalConfig,
    EditorConfig,
    ViewerConfig,
    SshConfig,
)> {
    let path = config_path().ok_or_else(|| anyhow!("Could not determine config directory"))?;
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
    let default_ssh = SshConfig {
        keepalive_interval: Some(3),
        read_timeout_secs: Some(5),
        watchdog_secs: Some(30),
    };
    let (keyboard, global, editor, viewer, ssh) = if path.exists() {
        let content =
            fs::read_to_string(&path).map_err(|e| anyhow!("Failed to read config file: {e}"))?;
        let user_config: AppConfig =
            toml::from_str(&content).map_err(|e| anyhow!("Config file is invalid: {e}"))?;
        let keyboard = merge_keyboard_config(user_config.keyboard.as_ref(), &default_keyboard);
        let global = merge_global_config(user_config.global.as_ref(), &default_global)?;
        let editor = user_config.editor.unwrap_or(default_editor);
        let viewer = user_config.viewer.unwrap_or(default_viewer);
        let ssh = user_config.ssh.unwrap_or(default_ssh);
        (keyboard, global, editor, viewer, ssh)
    } else {
        (
            default_keyboard,
            default_global,
            default_editor,
            default_viewer,
            default_ssh,
        )
    };

    validate_keyboard_config(&keyboard)?;
    validate_global_config(&global)?;
    validate_editor_config(&editor)?;
    validate_viewer_config(&viewer)?;
    validate_ssh_config(&ssh)?;

    Ok((keyboard, global, editor, viewer, ssh))
}

#[must_use]
pub fn config_path() -> Option<PathBuf> {
    crate::paths::config_path()
}

/// Creates a default configuration file.
///
/// # Errors
///
/// Returns an error if the config file already exists or cannot be created.
pub fn create_default_config() -> Result<PathBuf> {
    let path = config_path().ok_or_else(|| anyhow!("Could not determine config directory"))?;
    if path.exists() {
        return Err(anyhow!("Config file already exists at {}", path.display()));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| anyhow!("Failed to create config directory: {e}"))?;
    }

    let default_config = AppConfig {
        global: Some(default_global_config()),
        keyboard: Some(default_keyboard_config()),
        editor: Some(EditorConfig::default()),
        viewer: Some(ViewerConfig::default()),
        ssh: Some(SshConfig::default()),
    };

    let toml_content = toml::to_string_pretty(&default_config)
        .map_err(|e| anyhow!("Failed to serialize default config: {e}"))?;

    fs::write(&path, toml_content).map_err(|e| anyhow!("Failed to write config file: {e}"))?;

    Ok(path)
}
