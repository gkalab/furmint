/// Returns a display path with the user's home directory prefix replaced by `~`.
///
/// Paths not under the home directory (or when it cannot be determined) are
/// returned unchanged.
#[must_use]
pub fn replace_home_with_tilde(path: &std::path::Path) -> String {
    let Some(home) = std::env::home_dir() else {
        return path.to_string_lossy().to_string();
    };
    let Ok(rest) = path.strip_prefix(&home) else {
        return path.to_string_lossy().to_string();
    };
    if rest.as_os_str().is_empty() {
        return "~".to_string();
    }
    let mut display = std::path::PathBuf::from("~");
    display.push(rest);
    display.to_string_lossy().to_string()
}
