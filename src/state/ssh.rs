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
    pub confirmation: Option<crate::state::ConfirmationState>,
    pub history_list_offset: usize,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum SshField {
    ConnectionString,
    Name,
    Port,
    History,
}

impl SshConnectionState {
    #[must_use]
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
            confirmation: None,
            history_list_offset: 0,
        }
    }

    /// Deliberately keeps every field: dismissing the SSH dialog must not
    /// discard the connection string, because escaping the password prompt
    /// reopens this popup pre-filled (see `handlers::popup_ssh`). The popup is
    /// fully re-initialized on open instead.
    pub fn reset(&mut self) {
        self.is_visible = false;
    }
}

impl Default for SshConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

use secrecy::SecretString;

pub struct SshPasswordState {
    pub is_visible: bool,
    pub password: SecretString,
    pub host: String,
    pub user: String,
    pub session_id: String,
    pub error: Option<String>,
    pub cursor_position: usize,
    /// True when the password prompt was opened from a remote bookmark.
    /// In that case `Esc` dismisses the prompt instead of reopening the SSH
    /// connection dialog.
    pub from_bookmark: bool,
}

impl SshPasswordState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            password: SecretString::new(String::new().into()),
            host: String::new(),
            user: String::new(),
            session_id: String::new(),
            error: None,
            cursor_position: 0,
            from_bookmark: false,
        }
    }

    /// Deliberately keeps every field: `SshError::Auth` re-initializes host,
    /// user, password, error and cursor when the popup is (re)opened.
    pub fn reset(&mut self) {
        self.is_visible = false;
    }
}

impl Default for SshPasswordState {
    fn default() -> Self {
        Self::new()
    }
}
