use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SshConnectionInfo {
    pub name: Option<String>,
    pub connection_string: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    pub path: Option<String>,
}

impl SshConnectionInfo {
    pub fn display_string(&self) -> String {
        let conn = format!("{}@{}:{}", self.user, self.host, self.port);
        if let Some(name) = &self.name
            && !name.is_empty()
        {
            return format!("[{}] {}", name, conn);
        }
        conn
    }
}
pub struct SshConnectionHistory {
    pub connections: Vec<SshConnectionInfo>,
    path: PathBuf,
}
impl SshConnectionHistory {
    pub fn new() -> anyhow::Result<Self> {
        let mut path = directories::ProjectDirs::from("", "", "fm")
            .map(|dirs| dirs.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        if !path.exists() {
            let _ = fs::create_dir_all(&path);
        }
        path.push("ssh_history.json");
        let connections = if path.exists() {
            let content = fs::read_to_string(&path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self { connections, path })
    }
    pub fn add(&mut self, info: SshConnectionInfo) {
        // Remove duplicate if it exists based on connection string
        self.connections
            .retain(|c| c.connection_string != info.connection_string);
        // Add to the front
        self.connections.insert(0, info);
        // Limit to 50 entries
        self.connections.truncate(50);
        let _ = self.save();
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(&self.connections)?;
        fs::write(&self.path, content)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
}
