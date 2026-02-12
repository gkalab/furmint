use crate::app_state::tabs::{SortColumn, SortDirection};
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
    pub sort_column: Option<SortColumn>,
    pub sort_direction: Option<SortDirection>,
}

impl SshConnectionInfo {
    #[must_use]
    pub fn display_string(&self) -> String {
        let conn = format!("{}@{}:{}", self.user, self.host, self.port);
        if let Some(name) = &self.name
            && !name.is_empty()
        {
            return format!("[{name}] {conn}");
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
        let mut path = directories::ProjectDirs::from("", "", "fm").map_or_else(
            || PathBuf::from("."),
            |dirs| dirs.config_dir().to_path_buf(),
        );
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
    pub fn add(&mut self, mut info: SshConnectionInfo) {
        // Find existing match by user@host:port
        if let Some(pos) = self.connections.iter().position(|c| {
            c.host.to_lowercase() == info.host.to_lowercase()
                && c.user == info.user
                && c.port == info.port
        }) {
            let mut existing = self.connections.remove(pos);

            // Preserve sort settings
            if info.sort_column.is_none() {
                info.sort_column = existing.sort_column;
            }
            if info.sort_direction.is_none() {
                info.sort_direction = existing.sort_direction;
            }

            // Update with new info
            existing.connection_string = info.connection_string;
            existing.name = info.name;
            existing.path = info.path;

            // Add back at the top
            self.connections.insert(0, existing);
        } else {
            // New connection
            self.connections.insert(0, info);
        }

        let _ = self.save();
    }
    pub fn remove_at(&mut self, idx: usize) {
        if idx < self.connections.len() {
            self.connections.remove(idx);
            let _ = self.save();
        }
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(&self.connections)?;
        fs::write(&self.path, content)?;
        Ok(())
    }
    pub fn load(&mut self) -> anyhow::Result<()> {
        if self.path.exists() {
            let content = fs::read_to_string(&self.path)?;
            self.connections = serde_json::from_str(&content).unwrap_or_default();
        }
        Ok(())
    }

    pub fn update_sort_settings(
        &mut self,
        host: &str,
        user: &str,
        name: Option<&str>,
        sort_column: SortColumn,
        sort_direction: SortDirection,
    ) {
        let mut updated = false;
        for conn in &mut self.connections {
            let match_by_name = if let (Some(n1), Some(n2)) = (name, &conn.name) {
                n1 == n2
            } else {
                false
            };

            if match_by_name
                || (conn.host.to_lowercase() == host.to_lowercase() && conn.user == user)
            {
                conn.sort_column = Some(sort_column);
                conn.sort_direction = Some(sort_direction);
                updated = true;
            }
        }
        if updated {
            let _ = self.save();
        }
    }

    #[must_use]
    pub fn get_sort_settings(
        &self,
        host: &str,
        user: &str,
        name: Option<&str>,
    ) -> Option<(SortColumn, SortDirection)> {
        self.connections
            .iter()
            .find(|c| {
                // Try to match by name first if provided
                if let (Some(n1), Some(n2)) = (name, &c.name)
                    && n1 == n2
                    && c.sort_column.is_some()
                    && c.sort_direction.is_some()
                {
                    return true;
                }

                c.host.to_lowercase() == host.to_lowercase()
                    && c.user == user
                    && c.sort_column.is_some()
                    && c.sort_direction.is_some()
            })
            .and_then(|c| {
                if let (Some(col), Some(dir)) = (c.sort_column, c.sort_direction) {
                    Some((col, dir))
                } else {
                    None
                }
            })
    }
}
