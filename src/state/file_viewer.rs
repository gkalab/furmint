use std::path::PathBuf;

#[derive(Default)]
pub struct FileViewerState {
    pub path: PathBuf,
    pub content: Vec<String>,
    pub scroll_offset: usize,
    pub horizontal_scroll_offset: usize,
    pub is_visible: bool,
    pub syntax_set: syntect::parsing::SyntaxSet,
    pub theme: syntect::highlighting::Theme,
    pub syntax_name: Option<String>,
    pub focused: bool,
}

impl FileViewerState {
    pub fn new(is_dark_theme: bool, app_theme_name: &str) -> Self {
        let syntax_set = syntect::parsing::SyntaxSet::load_defaults_newlines();
        let theme_set = syntect::highlighting::ThemeSet::load_defaults();
        // Select theme based on app theme variant
        let theme_name = if app_theme_name == "solarized light" {
            "Solarized (light)"
        } else if is_dark_theme {
            "base16-eighties.dark"
        } else {
            "InspiredGitHub"
        };
        let theme = theme_set.themes[theme_name].clone();

        Self {
            path: PathBuf::new(),
            content: Vec::new(),
            scroll_offset: 0,
            horizontal_scroll_offset: 0,
            is_visible: false,
            syntax_set,
            theme,
            syntax_name: None,
            focused: false,
        }
    }

    pub fn load_content(&mut self, path: PathBuf) {
        self.path = path.clone();
        self.scroll_offset = 0;
        self.horizontal_scroll_offset = 0;

        // Determine syntax once
        let syntax = self
            .syntax_set
            .find_syntax_for_file(&self.path)
            .unwrap_or(None)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        self.syntax_name = Some(syntax.name.clone());

        match crate::fs_ops::read_file_content(&self.path, 10 * 1024 * 1024) {
            // 10MB limit
            Ok(content) => {
                self.content = content.lines().map(String::from).collect();
            }
            Err(e) => {
                self.content = vec![format!("Error reading file: {e}")];
            }
        }
    }
}
