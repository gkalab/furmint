use fm::config::{
    EditorConfig, GlobalConfig, KeyboardConfig, SshConfig, ViewerConfig, default_global_config,
    default_keyboard_config, merge_global_config, merge_keyboard_config, parse_command,
    validate_editor_config, validate_keyboard_config, validate_ssh_config, validate_viewer_config,
};
use fm::theme::THEME_NAMES;

#[test]
fn test_config_path_returns_some() {
    let path = fm::config::config_path();
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
    let valid_theme = THEME_NAMES.iter().next().unwrap().to_string();
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
    config.quit = Some(vec!["q".to_string()]);
    config.new_file = Some(vec!["q".to_string()]);
    let result = validate_keyboard_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Keybinding conflict")
    );
}

#[test]
fn test_validate_keyboard_config_same_action_no_conflict() {
    let mut config = default_keyboard_config();
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

    let abs_no_exist = if cfg!(windows) {
        "C:\\non\\existent\\path\\to\\editor"
    } else {
        "/non/existent/path/to/editor"
    };
    let editor_abs_no_exist = EditorConfig {
        command: Some(abs_no_exist.to_string()),
        in_terminal: Some(true),
    };
    assert!(validate_editor_config(&editor_abs_no_exist).is_err());

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
fn test_parse_command() {
    assert_eq!(parse_command("vim"), ("vim".to_string(), vec![]));

    assert_eq!(
        parse_command("vim -u NONE"),
        (
            "vim".to_string(),
            vec!["-u".to_string(), "NONE".to_string()]
        )
    );

    assert_eq!(
        parse_command("\"C:/Program Files/vim.exe\" -v"),
        (
            "C:/Program Files/vim.exe".to_string(),
            vec!["-v".to_string()]
        )
    );

    assert_eq!(
        parse_command("C:/This/Path/Definitely/Does/Not/Exist/my_editor"),
        (
            "C:/This/Path/Definitely/Does/Not/Exist/my_editor".to_string(),
            vec![]
        )
    );
}

#[test]
fn test_validate_ssh_config_valid() {
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: Some(15),
        watchdog_secs: Some(30),
    };
    assert!(validate_ssh_config(&config).is_ok());
}

#[test]
fn test_validate_ssh_config_invalid_keepalive_zero() {
    let config = SshConfig {
        keepalive_interval: Some(0),
        read_timeout_secs: Some(15),
        watchdog_secs: Some(30),
    };
    let result = validate_ssh_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("keepalive_interval must be greater than 0")
    );
}

#[test]
fn test_validate_ssh_config_invalid_keepalive_too_large() {
    let config = SshConfig {
        keepalive_interval: Some(4000),
        read_timeout_secs: Some(15),
        watchdog_secs: Some(30),
    };
    let result = validate_ssh_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("keepalive_interval must be less than or equal to 3600")
    );
}

#[test]
fn test_validate_ssh_config_invalid_timeout_zero() {
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: Some(0),
        watchdog_secs: Some(30),
    };
    let result = validate_ssh_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("read_timeout_secs must be greater than 0")
    );
}

#[test]
fn test_validate_ssh_config_invalid_watchdog_zero() {
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: Some(15),
        watchdog_secs: Some(0),
    };
    let result = validate_ssh_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("watchdog_secs must be greater than 0")
    );
}

#[test]
fn test_validate_ssh_config_timeout_greater_than_watchdog() {
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: Some(60),
        watchdog_secs: Some(30),
    };
    let result = validate_ssh_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("read_timeout_secs (60) must be less than or equal to watchdog_secs (30)")
    );
}

#[test]
fn test_validate_ssh_config_none_values() {
    let config = SshConfig {
        keepalive_interval: None,
        read_timeout_secs: None,
        watchdog_secs: None,
    };
    assert!(validate_ssh_config(&config).is_ok());
}

#[test]
fn test_validate_ssh_config_partial_values() {
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: None,
        watchdog_secs: None,
    };
    assert!(validate_ssh_config(&config).is_ok());
}
