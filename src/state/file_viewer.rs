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

    pub fn load_content(
        &mut self,
        path: PathBuf,
        provider: std::sync::Arc<dyn crate::fs_provider::FileSystemProvider>,
        size: Option<u64>,
        limit_bytes: u64,
    ) {
        self.path.clone_from(&path);
        self.scroll_offset = 0;
        self.horizontal_scroll_offset = 0;

        // Determine syntax once
        let syntax = self
            .syntax_set
            .find_syntax_for_file(&self.path)
            .unwrap_or(None)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        self.syntax_name = Some(syntax.name.clone());

        // Configurable file size limit (in bytes)
        let limit = limit_bytes as usize;
        let limit_u64 = limit_bytes;

        // Pre-check size if available
        if let Some(s) = size
            && s > limit_u64
        {
            self.content = vec![format!(
                "File too large to display (size: {}, limit: {})",
                crate::fs::utils::format_size(Some(s), false, false),
                crate::fs::utils::format_size(Some(limit_u64), false, false)
            )];
            return;
        }

        // Read small chunk to check for binary
        let chunk = match provider.read_file_at(&self.path, 0u64, 8192usize) {
            Ok(buf) => buf,
            Err(e) => {
                self.content = vec![format!("Error reading file: {e}")];
                return;
            }
        };
        if chunk.contains(&0) {
            self.content = vec!["Binary file detected".to_string()];
            return;
        }

        // Now safe to read content up to limit
        match provider.read_file_content(&self.path, limit) {
            Ok(content) => {
                self.content = content.lines().map(String::from).collect();
            }
            Err(e) => {
                self.content = vec![format!("Error reading file: {e}")];
            }
        }
    }
}
