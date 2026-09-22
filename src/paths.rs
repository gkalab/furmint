use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};

/// Returns the project directories for the application.
/// Uses a flat structure ("fm") to avoid nested organization folders on Windows.
#[must_use]
pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "fm")
}

/// Returns the path to the configuration file (config.toml).
/// Following `XDG_CONFIG_HOME` on Linux and Roaming `AppData` on Windows.
#[must_use]
pub fn config_path() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().join("config.toml"))
}

/// Returns the directory for persistent state files.
/// Following `XDG_STATE_HOME` on Linux (~/.local/state/fm).
/// On Windows, falls back to Local `AppData` (AppData\Local\fm\state).
#[must_use]
pub fn state_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| {
        dirs.state_dir().map_or_else(
            || {
                // Fallback for Windows/macOS where state_dir() is None
                #[cfg(windows)]
                {
                    dirs.data_local_dir().join("state")
                }
                #[cfg(not(windows))]
                {
                    // For other platforms (like macOS), fall back to data_dir/state
                    dirs.data_dir().join("state")
                }
            },
            std::path::Path::to_path_buf,
        )
    })
}

/// Atomically writes `content` to `path` by writing to a sibling temp file and
/// renaming it over the target. A crash mid-write leaves the previous file
/// intact instead of a truncated/corrupt one.
///
/// # Errors
///
/// Returns an error if the temp file cannot be written, the old file cannot be
/// removed, or the rename fails.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, content)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}
