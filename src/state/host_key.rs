use secrecy::SecretString;

pub struct HostKeyPrompt {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub presented_fp: String,
    pub stored_fp: Option<String>,
    pub key_line: String,
    pub password: Option<SecretString>,
    pub target_path: Option<String>,
    pub key_auth: bool,
    pub connection_name: Option<String>,
}

pub struct HostKeyState {
    pub is_visible: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub presented_fp: String,
    pub stored_fp: Option<String>,
    pub key_line: String,
    pub password: Option<SecretString>,
    pub target_path: Option<String>,
    pub key_auth: bool,
    pub connection_name: Option<String>,
    pub selected_no: bool,
    pub popup_area: ratatui::layout::Rect,
    pub button_areas: Vec<ratatui::layout::Rect>,
}

impl HostKeyState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_visible: false,
            host: String::new(),
            port: 22,
            user: String::new(),
            presented_fp: String::new(),
            stored_fp: None,
            key_line: String::new(),
            password: None,
            target_path: None,
            key_auth: false,
            connection_name: None,
            selected_no: true,
            popup_area: ratatui::layout::Rect::default(),
            button_areas: Vec::new(),
        }
    }

    pub fn show(&mut self, prompt: HostKeyPrompt) {
        self.host = prompt.host;
        self.port = prompt.port;
        self.user = prompt.user;
        self.presented_fp = prompt.presented_fp;
        self.stored_fp = prompt.stored_fp;
        self.key_line = prompt.key_line;
        self.password = prompt.password;
        self.target_path = prompt.target_path;
        self.key_auth = prompt.key_auth;
        self.connection_name = prompt.connection_name;
        self.selected_no = true;
        self.is_visible = true;
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
    }
}

impl Default for HostKeyState {
    fn default() -> Self {
        Self::new()
    }
}
