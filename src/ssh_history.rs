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
    pub path: PathBuf,
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
