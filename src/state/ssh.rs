use crate::ui::text_edit::{SecretEdit, TextEditState};

pub struct SshConnectionState {
    pub is_visible: bool,
    pub connection_string: TextEditState,
    pub name: TextEditState,
    pub port: TextEditState,
    pub active_field: SshField,
    pub error: Option<String>,
    pub selected_history_idx: Option<usize>,
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
            connection_string: TextEditState::new(),
            name: TextEditState::new(),
            port: TextEditState::with_text("22"),
            active_field: SshField::ConnectionString,
            error: None,
            selected_history_idx: None,
            search_query: String::new(),
            last_key_time: None,
            confirmation: None,
            history_list_offset: 0,
        }
    }

    /// Opens a fresh connection dialog, focused on the connection-string field
    /// and highlighting `selected_history_idx` (the first history entry, when
    /// there is one). Visibility is set here, so a caller cannot show the popup
    /// without populating it.
    pub fn show(&mut self, selected_history_idx: Option<usize>) {
        self.error = None;
        self.active_field = SshField::ConnectionString;
        self.connection_string.clear();
        self.name.clear();
        self.port.set_text("22");
        self.selected_history_idx = selected_history_idx;
        self.search_query.clear();
        self.last_key_time = None;
        self.is_visible = true;
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

pub struct SshPasswordState {
    pub is_visible: bool,
    pub password: SecretEdit,
    pub host: String,
    pub user: String,
    pub session_id: String,
    pub error: Option<String>,
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
            password: SecretEdit::new(),
            host: String::new(),
            user: String::new(),
            session_id: String::new(),
            error: None,
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
        self.password.clear();
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
mod tests;
