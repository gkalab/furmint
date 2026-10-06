use globset::{GlobBuilder, GlobMatcher};

/// Whether the platform's default filesystem is case-insensitive.
///
/// File name globs follow the same case sensitivity as the shell's globbing on
/// each platform: case-insensitive on Windows and macOS, case-sensitive on
/// Linux and other Unix systems.
#[cfg(any(windows, target_os = "macos"))]
const CASE_INSENSITIVE: bool = true;
#[cfg(not(any(windows, target_os = "macos")))]
const CASE_INSENSITIVE: bool = false;

/// Compile a file name glob into a matcher.
///
/// Supported syntax (identical on all platforms):
/// * `*` matches any run of characters
/// * `?` matches exactly one character
/// * `[abc]`, `[a-z]`, `[!abc]` match one character from a set
/// * `{a,b}` matches any of the alternatives
/// * `\` escapes the next character
///
/// The pattern is matched against a single entry name, so `*.zip` matches every
/// zip archive in the current directory.
///
/// Patterns entered by the user go through [`effective_glob`], which wraps
/// them in implicit wildcards before compilation.
///
/// # Errors
///
/// Returns `globset::Error` if the pattern is not a valid glob.
pub fn compile_glob(pattern: &str) -> Result<GlobMatcher, globset::Error> {
    Ok(GlobBuilder::new(pattern)
        .case_insensitive(CASE_INSENSITIVE)
        .build()?
        .compile_matcher())
}

/// Turn a user-entered filter pattern into the effective glob.
///
/// The pattern is implicitly wrapped in `*...*`, so typing `jpg` matches every
/// entry name containing `jpg`.
#[must_use]
pub fn effective_glob(pattern: &str) -> String {
    format!("*{pattern}*")
}

#[derive(Clone)]
pub struct FileFilterState {
    pub active: bool,
    pub pattern: String,
    pub cursor_position: usize,
    pub previous_filter: Option<String>,
    /// The currently applied filter pattern (if any).
    pub applied: Option<String>,
    /// Compiled glob for the applied filter.
    pub glob: Option<GlobMatcher>,
}

impl FileFilterState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            active: false,
            pattern: String::new(),
            cursor_position: 0,
            previous_filter: None,
            applied: None,
            glob: None,
        }
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.pattern.clear();
        self.cursor_position = 0;
        self.previous_filter = None;
    }

    /// Set the active filter pattern. Empty or `None` clears the filter.
    ///
    /// The pattern is a glob that gets implicitly wrapped in `*...*`
    /// (see [`effective_glob`]) before matching. The raw pattern is kept in
    /// [`Self::applied`] for display.
    ///
    /// # Errors
    ///
    /// Returns `globset::Error` if the pattern is not a valid glob.
    pub fn set(&mut self, pattern: Option<&str>) -> Result<(), globset::Error> {
        match pattern {
            None | Some("") => {
                self.applied = None;
                self.glob = None;
            }
            Some(p) => {
                let glob = compile_glob(&effective_glob(p))?;
                self.applied = Some(p.to_string());
                self.glob = Some(glob);
            }
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.applied = None;
        self.glob = None;
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.applied.is_some()
    }
}

impl Default for FileFilterState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, name: &str) -> bool {
        compile_glob(pattern).unwrap().is_match(name)
    }

    #[test]
    fn test_new_and_default() {
        let state = FileFilterState::new();
        assert!(!state.active);
        assert_eq!(state.pattern, "");
        assert_eq!(state.cursor_position, 0);
        assert!(state.previous_filter.is_none());
        assert!(state.applied.is_none());
        assert!(state.glob.is_none());

        let default_state = FileFilterState::default();
        assert_eq!(state.active, default_state.active);
        assert_eq!(state.pattern, default_state.pattern);
    }

    #[test]
    fn test_set_valid_glob() {
        let mut state = FileFilterState::new();
        assert!(state.set(Some("*.zip")).is_ok());
        assert!(state.is_active());
        assert_eq!(state.applied, Some("*.zip".to_string()));
        assert!(state.glob.is_some());
    }

    #[test]
    fn test_set_invalid_glob() {
        let mut state = FileFilterState::new();
        let result = state.set(Some("a{b"));
        assert!(result.is_err());
        assert!(state.applied.is_none());
        assert!(state.glob.is_none());
    }

    #[test]
    fn test_set_empty_clears_filter() {
        let mut state = FileFilterState::new();
        let _ = state.set(Some("*.zip"));
        assert!(state.is_active());
        state.set(Some("")).unwrap();
        assert!(!state.is_active());
        assert!(state.applied.is_none());
    }

    #[test]
    fn test_set_none_clears_filter() {
        let mut state = FileFilterState::new();
        let _ = state.set(Some("*.zip"));
        assert!(state.is_active());
        state.set(None).unwrap();
        assert!(!state.is_active());
    }

    #[test]
    fn test_clear() {
        let mut state = FileFilterState::new();
        let _ = state.set(Some("*.zip"));
        state.clear();
        assert!(state.applied.is_none());
        assert!(state.glob.is_none());
    }

    #[test]
    fn test_reset() {
        let mut state = FileFilterState {
            active: true,
            pattern: "*.zip".to_string(),
            cursor_position: 2,
            previous_filter: Some("*.rs".to_string()),
            applied: Some("*.rs".to_string()),
            glob: Some(compile_glob("*.rs").unwrap()),
        };
        state.reset();
        assert!(!state.active);
        assert_eq!(state.pattern, "");
        assert_eq!(state.cursor_position, 0);
        assert!(state.previous_filter.is_none());
    }

    #[test]
    fn test_is_active() {
        let mut state = FileFilterState::new();
        assert!(!state.is_active());
        let _ = state.set(Some("*.zip"));
        assert!(state.is_active());
    }

    #[test]
    fn test_effective_glob_wraps_pattern() {
        assert_eq!(effective_glob("jpg"), "*jpg*");
        assert_eq!(effective_glob("*.zip"), "**.zip*");
    }

    #[test]
    fn test_set_wraps_pattern_implicitly() {
        let mut state = FileFilterState::new();
        state.set(Some("jpg")).unwrap();
        let glob = state.glob.as_ref().unwrap();
        assert!(glob.is_match("photo.jpg"));
        assert!(glob.is_match("jpg"));
        assert!(!glob.is_match("jpeg"));
        // Display keeps the raw user pattern
        assert_eq!(state.applied, Some("jpg".to_string()));
    }

    #[test]
    fn test_star_glob() {
        assert!(matches("*.zip", "archive.zip"));
        assert!(!matches("*.zip", "archive.rar"));
        assert!(!matches("*.zip", "zip"));
    }

    #[test]
    fn test_star_matches_empty() {
        assert!(matches("*", "anything"));
        assert!(matches("*", ""));
    }

    #[test]
    fn test_prefix_glob() {
        assert!(matches("src*", "src"));
        assert!(matches("src*", "srcs"));
        assert!(!matches("src*", "lib"));
    }

    #[test]
    fn test_question_mark_glob() {
        assert!(matches("?.txt", "a.txt"));
        assert!(!matches("?.txt", "ab.txt"));
    }

    #[test]
    fn test_character_class_glob() {
        assert!(matches("file[0-9].txt", "file7.txt"));
        assert!(!matches("file[0-9].txt", "filex.txt"));
        assert!(matches("file[!0-9].txt", "filex.txt"));
    }

    #[test]
    fn test_alternates_glob() {
        assert!(matches("*.{zip,7z}", "a.zip"));
        assert!(matches("*.{zip,7z}", "a.7z"));
        assert!(!matches("*.{zip,7z}", "a.rar"));
    }

    #[test]
    fn test_escape_glob() {
        assert!(matches(r"a\*b", "a*b"));
        assert!(!matches(r"a\*b", "axb"));
    }

    #[test]
    fn test_case_sensitivity_follows_platform() {
        assert_eq!(matches("*.ZIP", "a.zip"), CASE_INSENSITIVE);
        assert_eq!(matches("*.zip", "a.ZIP"), CASE_INSENSITIVE);
    }
}
