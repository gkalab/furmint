pub struct SshConnectionState {
    pub is_visible: bool,
    pub connection_string: String,
    pub name: String,
    pub port: String,
    pub active_field: SshField,
    pub error: Option<String>,
    pub selected_history_idx: Option<usize>,
    pub cursor_position: usize,
    pub search_query: String,
    pub last_key_time: Option<std::time::Instant>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum SshField {
    ConnectionString,
    Name,
    Port,
    History,
}

impl SshConnectionState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            connection_string: String::new(),
            name: String::new(),
            port: "22".to_string(),
            active_field: SshField::ConnectionString,
            error: None,
            selected_history_idx: None,
            cursor_position: 0,
            search_query: String::new(),
            last_key_time: None,
        }
    }
}

pub struct SshPasswordState {
    pub is_visible: bool,
    pub password: String,
    pub host: String, // Context info to show in popup
    pub user: String,
}

impl SshPasswordState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            password: String::new(),
            host: String::new(),
            user: String::new(),
        }
    }
}
