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
    // Explicitly enable `\` as an escape on all platforms: globset disables
    // it by default on Windows (where `\` is a path separator) and would
    // otherwise treat it as one.
    Ok(GlobBuilder::new(pattern)
        .case_insensitive(CASE_INSENSITIVE)
        .backslash_escape(true)
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
mod tests;
