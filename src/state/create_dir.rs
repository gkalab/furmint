pub struct CreateDirectoryState {
    pub is_visible: bool,
    pub new_name: String,
    pub cursor_position: usize,
    pub error: Option<String>,
}

impl CreateDirectoryState {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            new_name: String::new(),
            cursor_position: 0,
            error: None,
        }
    }

    pub fn reset(&mut self) {
        self.is_visible = false;
        self.new_name.clear();
        self.cursor_position = 0;
        self.error = None;
    }
}

impl Default for CreateDirectoryState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_and_default() {
        let s1 = CreateDirectoryState::new();
        let s2 = CreateDirectoryState::default();
        assert!(!s1.is_visible);
        assert_eq!(s1.new_name, "");
        assert_eq!(s1.cursor_position, 0);
        assert_eq!(s1.error, None);
        // Default state matches new
        assert_eq!(s1.is_visible, s2.is_visible);
        assert_eq!(s1.new_name, s2.new_name);
        assert_eq!(s1.cursor_position, s2.cursor_position);
        assert_eq!(s1.error, s2.error);
    }

    #[test]
    fn test_reset() {
        let mut st = CreateDirectoryState {
            is_visible: true,
            new_name: "testdir".into(),
            cursor_position: 42,
            error: Some("err".into()),
        };
        st.reset();
        assert!(!st.is_visible);
        assert_eq!(st.new_name, "");
        assert_eq!(st.cursor_position, 0);
        assert_eq!(st.error, None);
    }
}
