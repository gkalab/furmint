#[derive(Clone)]
pub struct FileFilterState {
    pub active: bool,
    pub pattern: String,
    pub cursor_position: usize,
    pub previous_filter: Option<String>,
    /// The currently applied filter pattern (if any).
    pub applied: Option<String>,
    /// Compiled regex for the applied filter.
    pub regex: Option<regex::Regex>,
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
            regex: None,
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
    /// # Errors
    ///
    /// Returns `regex::Error` if the pattern is not a valid regex.
    pub fn set(&mut self, pattern: Option<&str>) -> Result<(), regex::Error> {
        match pattern {
            None | Some("") => {
                self.applied = None;
                self.regex = None;
            }
            Some(p) => {
                let re = regex::Regex::new(p)?;
                self.applied = Some(p.to_string());
                self.regex = Some(re);
            }
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.applied = None;
        self.regex = None;
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
