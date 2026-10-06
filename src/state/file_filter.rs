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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_and_default() {
        let state = FileFilterState::new();
        assert!(!state.active);
        assert_eq!(state.pattern, "");
        assert_eq!(state.cursor_position, 0);
        assert!(state.previous_filter.is_none());
        assert!(state.applied.is_none());
        assert!(state.regex.is_none());

        let default_state = FileFilterState::default();
        assert_eq!(state.active, default_state.active);
        assert_eq!(state.pattern, default_state.pattern);
    }

    #[test]
    fn test_set_valid_regex() {
        let mut state = FileFilterState::new();
        assert!(state.set(Some("foo")).is_ok());
        assert!(state.is_active());
        assert_eq!(state.applied, Some("foo".to_string()));
        assert!(state.regex.is_some());
    }

    #[test]
    fn test_set_invalid_regex() {
        let mut state = FileFilterState::new();
        let result = state.set(Some("[invalid"));
        assert!(result.is_err());
        assert!(state.applied.is_none());
        assert!(state.regex.is_none());
    }

    #[test]
    fn test_set_empty_clears_filter() {
        let mut state = FileFilterState::new();
        let _ = state.set(Some("foo"));
        assert!(state.is_active());
        state.set(Some("")).unwrap();
        assert!(!state.is_active());
        assert!(state.applied.is_none());
    }

    #[test]
    fn test_set_none_clears_filter() {
        let mut state = FileFilterState::new();
        let _ = state.set(Some("foo"));
        assert!(state.is_active());
        state.set(None).unwrap();
        assert!(!state.is_active());
    }

    #[test]
    fn test_clear() {
        let mut state = FileFilterState::new();
        let _ = state.set(Some("foo"));
        state.clear();
        assert!(state.applied.is_none());
        assert!(state.regex.is_none());
    }

    #[test]
    fn test_reset() {
        let mut state = FileFilterState {
            active: true,
            pattern: "foo".to_string(),
            cursor_position: 2,
            previous_filter: Some("bar".to_string()),
            applied: Some("baz".to_string()),
            regex: Some(regex::Regex::new("baz").unwrap()),
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
        let _ = state.set(Some("foo"));
        assert!(state.is_active());
    }
}
