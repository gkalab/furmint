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

    /// Opens the dialog showing `error`, focused on the connection-string
    /// field. Visibility is set here, so a caller cannot show the popup without
    /// populating it.
    pub fn show_error(&mut self, error: String) {
        self.error = Some(error);
        self.active_field = SshField::ConnectionString;
        self.is_visible = true;
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

    /// Opens the password prompt for `session_id`/`host`/`user`, clearing any
    /// previously typed password and positioning the cursor. Visibility is set
    /// here, so a caller cannot show the popup without populating it.
    pub fn show(&mut self, session_id: String, host: String, user: String, error: Option<String>) {
        self.session_id = session_id;
        self.host = host;
        self.user = user;
        self.error = error;
        self.password = SecretString::new(String::new().into());
        self.cursor_position = 0;
        self.is_visible = true;
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

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn password_show_populates_and_makes_visible() {
        let mut state = SshPasswordState::new();
        state.password = secrecy::SecretString::new("stale".to_string().into());
        state.show(
            "sid".to_string(),
            "host".to_string(),
            "user".to_string(),
            Some("bad password".to_string()),
        );

        assert!(state.is_visible);
        assert_eq!(state.session_id, "sid");
        assert_eq!(state.host, "host");
        assert_eq!(state.user, "user");
        assert_eq!(state.error.as_deref(), Some("bad password"));
        assert_eq!(state.password.expose_secret(), "");
        assert_eq!(state.cursor_position, 0);
    }

    #[test]
    fn connection_show_error_populates_and_makes_visible() {
        let mut state = SshConnectionState::new();
        state.active_field = SshField::History;
        state.show_error("no route to host".to_string());

        assert!(state.is_visible);
        assert_eq!(state.error.as_deref(), Some("no route to host"));
        assert_eq!(state.active_field, SshField::ConnectionString);
    }
}
