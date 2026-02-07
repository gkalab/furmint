use fm::app_state::tabs::{SortColumn, SortDirection};
use fm::ssh_history::{SshConnectionHistory, SshConnectionInfo};
use tempfile::tempdir;

fn create_test_info(conn: &str) -> SshConnectionInfo {
    SshConnectionInfo {
        name: None,
        connection_string: conn.to_string(),
        user: "root".to_string(),
        host: conn.to_string(),
        port: 22,
        path: None,
        sort_column: None,
        sort_direction: None,
    }
}

#[test]
fn test_history_deduplication() {
    let temp = tempdir().unwrap();
    let history_path = temp.path().join("ssh_history.json");
    let mut history = SshConnectionHistory {
        connections: Vec::new(),
        path: history_path,
    };

    history.add(create_test_info("host1"));
    history.add(create_test_info("host2"));
    history.add(create_test_info("host1")); // Duplicate

    assert_eq!(history.connections.len(), 2);
    assert_eq!(history.connections[0].connection_string, "host1");
    assert_eq!(history.connections[1].connection_string, "host2");
}

#[test]
fn test_display_string() {
    let info = SshConnectionInfo {
        name: Some("MyServer".to_string()),
        connection_string: "user@host:2277".to_string(),
        user: "user".to_string(),
        host: "host".to_string(),
        port: 2277,
        path: None,
        sort_column: None,
        sort_direction: None,
    };
    assert_eq!(info.display_string(), "[MyServer] user@host:2277");

    let info_no_name = SshConnectionInfo { name: None, ..info };
    assert_eq!(info_no_name.display_string(), "user@host:2277");
}

#[test]
fn test_sort_settings_persistence() {
    let temp = tempdir().unwrap();
    let history_path = temp.path().join("ssh_history.json");
    let mut history = SshConnectionHistory {
        connections: Vec::new(),
        path: history_path.clone(),
    };

    let info = SshConnectionInfo {
        name: None,
        connection_string: "user@host".to_string(),
        user: "user".to_string(),
        host: "host".to_string(),
        port: 22,
        path: None,
        sort_column: None,
        sort_direction: None,
    };
    history.add(info);

    // Update sort settings
    history.update_sort_settings(
        "host",
        "user",
        None,
        SortColumn::Size,
        SortDirection::Descending,
    );

    // Reload history
    let mut new_history = SshConnectionHistory {
        connections: Vec::new(),
        path: history_path,
    };
    new_history.load().unwrap();

    let settings = new_history.get_sort_settings("host", "user", None).unwrap();
    assert_eq!(settings.0, SortColumn::Size);
    assert_eq!(settings.1, SortDirection::Descending);
}

#[test]
fn test_context_key_extraction() {
    let context_key = "[user@host]";
    assert!(context_key.starts_with('[') && context_key.ends_with(']'));
    let inner = &context_key[1..context_key.len() - 1];
    let at_idx = inner.find('@').unwrap();
    let user = &inner[..at_idx];
    let host = &inner[at_idx + 1..];
    assert_eq!(user, "user");
    assert_eq!(host, "host");
}

#[test]
fn test_get_sort_settings_with_multiple_ports() {
    let temp = tempdir().unwrap();
    let history_path = temp.path().join("ssh_history.json");
    let mut history = SshConnectionHistory {
        connections: Vec::new(),
        path: history_path.clone(),
    };

    let info1 = SshConnectionInfo {
        name: None,
        connection_string: "user@host:22".to_string(),
        user: "user".to_string(),
        host: "host".to_string(),
        port: 22,
        path: None,
        sort_column: Some(SortColumn::Size),
        sort_direction: Some(SortDirection::Descending),
    };
    let info2 = SshConnectionInfo {
        name: None,
        connection_string: "user@host:2222".to_string(),
        user: "user".to_string(),
        host: "host".to_string(),
        port: 2222,
        path: None,
        sort_column: None,
        sort_direction: None,
    };
    history.add(info1);
    history.add(info2);

    // It should find the one that has settings, even if it's not the first one.
    assert!(history.get_sort_settings("host", "user", None).is_some());
    let settings = history.get_sort_settings("host", "user", None).unwrap();
    assert_eq!(settings.0, SortColumn::Size);
    assert_eq!(settings.1, SortDirection::Descending);
}

#[test]
fn test_user_reported_scenario() {
    // Replicate exactly what the user has in their history JSON
    let temp = tempdir().unwrap();
    let history_path = temp.path().join("ssh_history.json");
    let mut history = SshConnectionHistory {
        connections: Vec::new(),
        path: history_path.clone(),
    };

    let info = SshConnectionInfo {
        name: Some("somehost".to_string()),
        connection_string: "192.168.1.1".to_string(),
        user: "root".to_string(),
        host: "192.168.1.1".to_string(),
        port: 22,
        path: None,
        sort_column: Some(SortColumn::Date),
        sort_direction: Some(SortDirection::Descending),
    };
    history.add(info);

    // Simulate extraction logic from handle_ssh_connected
    let context_key = "[root@192.168.1.1]";
    assert!(context_key.starts_with('[') && context_key.ends_with(']'));
    let inner = &context_key[1..context_key.len() - 1];
    let at_idx = inner.find('@').expect("Forgot @ symbol in key");
    let user = &inner[..at_idx];
    let host = &inner[at_idx + 1..];

    assert_eq!(user, "root");
    assert_eq!(host, "192.168.1.1");

    let settings = history.get_sort_settings(host, user, None);
    assert!(
        settings.is_some(),
        "Settings should be found for user's entry"
    );
    let (col, dir) = settings.unwrap();
    assert_eq!(col, SortColumn::Date);
    assert_eq!(dir, SortDirection::Descending);
}

#[test]
fn test_add_preserves_sort_settings() {
    let temp = tempdir().unwrap();
    let history_path = temp.path().join("ssh_history.json");
    let mut history = SshConnectionHistory {
        connections: Vec::new(),
        path: history_path,
    };

    let mut info = SshConnectionInfo {
        name: Some("test".to_string()),
        connection_string: "user@host".to_string(),
        user: "user".to_string(),
        host: "host".to_string(),
        port: 22,
        path: None,
        sort_column: Some(SortColumn::Size),
        sort_direction: Some(SortDirection::Descending),
    };
    history.add(info.clone());

    // Connect manually again (overwrites with None in the call)
    info.sort_column = None;
    info.sort_direction = None;
    history.add(info);

    let settings = history
        .get_sort_settings("host", "user", Some("test"))
        .unwrap();
    assert_eq!(settings.0, SortColumn::Size);
    assert_eq!(settings.1, SortDirection::Descending);
}

#[test]
fn test_get_sort_settings_by_name() {
    let temp = tempdir().unwrap();
    let history_path = temp.path().join("ssh_history.json");
    let mut history = SshConnectionHistory {
        connections: Vec::new(),
        path: history_path,
    };

    let info = SshConnectionInfo {
        name: Some("alias".to_string()),
        connection_string: "alias".to_string(),
        user: "root".to_string(),
        host: "10.0.0.1".to_string(),
        port: 22,
        path: None,
        sort_column: Some(SortColumn::Extension),
        sort_direction: Some(SortDirection::Ascending),
    };
    history.add(info);

    // Should match by name even if host/user are passed but might be different
    let settings = history.get_sort_settings("incorrect_host", "incorrect_user", Some("alias"));
    assert!(settings.is_some(), "Should find settings by name");
    assert_eq!(settings.unwrap().0, SortColumn::Extension);
}

#[test]
fn test_ssh_history_deduplication() {
    let tmp = tempdir().unwrap();
    let mut history = SshConnectionHistory {
        connections: Vec::new(),
        path: tmp.path().join("ssh_history.json"),
    };

    let info1 = SshConnectionInfo {
        name: None,
        connection_string: "user@host".to_string(),
        user: "user".to_string(),
        host: "host".to_string(),
        port: 22,
        path: None,
        sort_column: None,
        sort_direction: None,
    };

    history.add(info1);
    assert_eq!(history.connections.len(), 1);

    // Same connection, different path
    let info2 = SshConnectionInfo {
        name: None,
        connection_string: "user@host:/path".to_string(),
        user: "user".to_string(),
        host: "host".to_string(),
        port: 22,
        path: Some("/path".to_string()),
        sort_column: None,
        sort_direction: None,
    };

    history.add(info2);
    assert_eq!(history.connections.len(), 1);
    assert_eq!(history.connections[0].path, Some("/path".to_string()));
    assert_eq!(history.connections[0].connection_string, "user@host:/path");

    // Same connection, explicit root
    let info3 = SshConnectionInfo {
        name: None,
        connection_string: "root@host".to_string(),
        user: "root".to_string(),
        host: "host".to_string(),
        port: 22,
        path: None,
        sort_column: None,
        sort_direction: None,
    };
    history.add(info3);

    let info4 = SshConnectionInfo {
        name: None,
        connection_string: "host".to_string(),
        user: "root".to_string(),
        host: "host".to_string(),
        port: 22,
        path: None,
        sort_column: None,
        sort_direction: None,
    };
    history.add(info4);
    assert_eq!(history.connections.len(), 2); // user@host and root@host

    // Find positions
    let root_pos = history
        .connections
        .iter()
        .position(|c| c.user == "root")
        .unwrap();
    assert_eq!(root_pos, 0); // Most recent at top
}
