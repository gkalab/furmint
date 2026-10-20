use std::path::PathBuf;

/// Strips the Windows extended-length prefix returned by
/// `std::fs::canonicalize` (\\?\D:\...) so paths look like normal DOS paths.
/// UNC paths (\\?\UNC\server\share) are converted to \\server\share.
#[must_use]
#[cfg(target_os = "windows")]
pub fn strip_extended_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

#[must_use]
#[cfg(not(target_os = "windows"))]
pub fn strip_extended_prefix(path: PathBuf) -> PathBuf {
    path
}
