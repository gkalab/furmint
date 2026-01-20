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
fn test_history_limit() {
    let temp = tempdir().unwrap();
    let history_path = temp.path().join("ssh_history.json");
    let mut history = SshConnectionHistory {
        connections: Vec::new(),
        path: history_path,
    };

    for i in 0..60 {
        history.add(create_test_info(&format!("host{}", i)));
    }

    assert_eq!(history.connections.len(), 50);
    assert_eq!(history.connections[0].connection_string, "host59");
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
    };
    assert_eq!(info.display_string(), "[MyServer] user@host:2277");

    let info_no_name = SshConnectionInfo { name: None, ..info };
    assert_eq!(info_no_name.display_string(), "user@host:2277");
}
