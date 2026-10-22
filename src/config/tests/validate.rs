use crate::config::{
    EditorConfig, SshConfig, SshTimeouts, ViewerConfig, default_global_config,
    default_keyboard_config, validate_editor_config, validate_keyboard_config, validate_ssh_config,
    validate_viewer_config,
};
use crate::theme::{DEFAULT_THEME_NAME, THEME_NAMES};

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
fn test_toggle_hidden_default_binding() {
    let config = default_keyboard_config();
    assert_eq!(
        config.toggle_hidden,
        Some(vec!["Alt-.".to_string()]),
        "hidden-file toggle defaults to Alt-."
    );
}

#[test]
fn test_toggle_hidden_conflict_is_detected() {
    let mut config = default_keyboard_config();
    config.toggle_hidden = Some(vec!["Ctrl-q".to_string()]);
    let err = validate_keyboard_config(&config).unwrap_err().to_string();
    assert!(err.contains("Keybinding conflict"), "{err}");
    assert!(err.contains("toggle_hidden"), "{err}");
}

#[test]
fn test_validate_editor_viewer_config() {
    let editor = EditorConfig {
        command: Some("vim".to_string()),
        in_terminal: Some(true),
    };
    assert!(validate_editor_config(&editor).is_ok());

    let editor_empty = EditorConfig {
        command: Some(String::new()),
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
fn test_validate_ssh_config_valid() {
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: Some(15),
        connect_timeout_secs: Some(30),
    };
    assert!(validate_ssh_config(&config).is_ok());
}

#[test]
fn test_validate_ssh_config_invalid_keepalive_zero() {
    let config = SshConfig {
        keepalive_interval: Some(0),
        read_timeout_secs: Some(15),
        connect_timeout_secs: Some(30),
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
        connect_timeout_secs: Some(30),
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
        connect_timeout_secs: Some(30),
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
fn test_validate_ssh_config_invalid_connect_timeout_zero() {
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: Some(15),
        connect_timeout_secs: Some(0),
    };
    let result = validate_ssh_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("connect_timeout_secs must be greater than 0")
    );
}

#[test]
fn test_validate_ssh_config_independent_timeouts() {
    // `read_timeout_secs` is a per-session inactivity timeout and
    // `connect_timeout_secs` bounds the initial dial; they are independent, so a
    // long read timeout with a short connect timeout is legitimate.
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: Some(600),
        connect_timeout_secs: Some(30),
    };
    assert!(validate_ssh_config(&config).is_ok());
}

#[test]
fn test_validate_ssh_config_none_values() {
    let config = SshConfig {
        keepalive_interval: None,
        read_timeout_secs: None,
        connect_timeout_secs: None,
    };
    assert!(validate_ssh_config(&config).is_ok());
}

#[test]
fn test_validate_ssh_config_partial_values() {
    let config = SshConfig {
        keepalive_interval: Some(10),
        read_timeout_secs: None,
        connect_timeout_secs: None,
    };
    assert!(validate_ssh_config(&config).is_ok());
}

#[test]
fn test_ssh_config_default_has_concrete_values() {
    // `create_default_config` serializes `SshConfig::default()`, so a derived
    // all-`None` `Default` would write an empty `[ssh]` table that documents
    // nothing about the values actually in effect.
    let config = SshConfig::default();
    assert!(config.keepalive_interval.is_some());
    assert!(config.read_timeout_secs.is_some());
    assert!(config.connect_timeout_secs.is_some());
}

#[test]
fn test_ssh_config_resolve_matches_default() {
    // An empty `[ssh]` table must resolve to the same numbers as no table at
    // all, so config load and `SshManager` agree on one default set.
    assert_eq!(SshConfig::default().resolve(), SshTimeouts::default());
    assert_eq!(SshConfig::default().resolve().connect_timeout_secs, 30);
}

#[test]
fn test_default_theme_name_is_a_valid_theme() {
    assert!(THEME_NAMES.contains(&DEFAULT_THEME_NAME));
    assert_eq!(
        default_global_config().theme.as_deref(),
        Some(DEFAULT_THEME_NAME)
    );
}
