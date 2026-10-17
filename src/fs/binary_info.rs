//! Describe binary files with the system `file` tool.

use std::path::Path;
use std::process::Command;

/// The `file` command's name on this platform.
#[cfg(windows)]
pub const FILE_COMMAND: &str = "file.exe";
/// The `file` command's name on this platform.
#[cfg(not(windows))]
pub const FILE_COMMAND: &str = "file";

/// Returns `file`'s description of `path`.
///
/// `None` means the tool is not installed, could not read the file, or printed
/// nothing useful; the caller should fall back to its own message. The command
/// blocks, so callers on the event loop should run it through
/// [`tokio::task::spawn_blocking`].
#[must_use]
pub fn describe(path: &Path) -> Option<String> {
    // `file` reports an unreadable path on stdout with a success status, so
    // check first and let the caller keep its own message.
    if !path.is_file() {
        return None;
    }
    // `-b` drops the leading "path: " so the description reads well on its own
    // (the viewer already shows the path in its title).
    let output = Command::new(FILE_COMMAND)
        .arg("-b")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When `file` is installed it describes a readable file; when it is not,
    /// `describe` reports that so the caller can fall back to its own message.
    #[test]
    fn describe_reports_a_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.txt");
        std::fs::write(&path, "hello").unwrap();

        if let Some(text) = describe(&path) {
            let lower = text.to_lowercase();
            assert!(
                lower.contains("text") || lower.contains("ascii"),
                "unexpected `file` output: {text}"
            );
        }
    }
}
