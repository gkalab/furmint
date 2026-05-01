use directories::ProjectDirs;
use std::path::PathBuf;

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
