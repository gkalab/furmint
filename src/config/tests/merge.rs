use crate::config::{
    GlobalConfig, KeyboardConfig, SshConfig, default_global_config, default_keyboard_config,
    merge_global_config, merge_keyboard_config, merge_ssh_config, validate_keyboard_config,
};
use crate::theme::THEME_NAMES;

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
    let valid_theme = (*THEME_NAMES.iter().next().unwrap()).to_string();
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
fn test_toggle_hidden_can_be_rebound() {
    let user = KeyboardConfig {
        toggle_hidden: Some(vec!["Ctrl-h".to_string()]),
        ..KeyboardConfig::default()
    };
    let merged = merge_keyboard_config(Some(&user), &default_keyboard_config());
    assert_eq!(merged.toggle_hidden, Some(vec!["Ctrl-h".to_string()]));
    assert!(validate_keyboard_config(&merged).is_ok());
}

#[test]
fn test_merge_ssh_config_fills_omitted_fields() {
    let user = SshConfig {
        keepalive_interval: None,
        read_timeout_secs: Some(120),
        connect_timeout_secs: None,
    };
    let default = SshConfig::default();
    let merged = merge_ssh_config(Some(&user), &default);

    assert_eq!(merged.read_timeout_secs, Some(120));
    assert_eq!(merged.keepalive_interval, default.keepalive_interval);
    assert_eq!(merged.connect_timeout_secs, default.connect_timeout_secs);
}

#[test]
fn test_merge_ssh_config_without_user_table() {
    let default = SshConfig::default();
    assert_eq!(merge_ssh_config(None, &default).read_timeout_secs, Some(5));
}
