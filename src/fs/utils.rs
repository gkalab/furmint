mod entry;
mod hidden;
mod listing;
mod paths;
mod ssh_keys;
mod trash;

pub use entry::*;
pub use hidden::*;
pub use listing::*;
pub use paths::*;
pub use ssh_keys::*;
pub use trash::*;

/// Returns an optimal chunk size for SFTP transfers based on the total file size.
#[must_use]
pub fn calculate_optimal_chunk_size(file_size: u64) -> usize {
    match file_size {
        0..=512_000 => 512 * 1024,              // 512KB for tiny files (≤512KB)
        512_001..=8_000_000 => 2 * 1024 * 1024, // 2MB for small files (512KB-8MB)
        8_000_001..=200_000_000 => 8 * 1024 * 1024, // 8MB for medium files (8MB-200MB)
        _ => 16 * 1024 * 1024,                  // 16MB for large files (>200MB)
    }
}

/// Formats a Unix permission mode into a standard 10-character string (e.g., "-rwxr-xr-x").
#[must_use]
pub fn format_sftp_permissions(perm: u32) -> String {
    let mut s = String::with_capacity(10);
    // Type (simplified mask check)
    s.push(if (perm & 0o170_000) == 0o040_000 {
        'd'
    } else if (perm & 0o170_000) == 0o120_000 {
        'l'
    } else {
        '-'
    });
    // User
    s.push(if perm & 0o400 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o200 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o100 != 0 { 'x' } else { '-' });
    // Group
    s.push(if perm & 0o040 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o020 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o010 != 0 { 'x' } else { '-' });
    // Others
    s.push(if perm & 0o004 != 0 { 'r' } else { '-' });
    s.push(if perm & 0o002 != 0 { 'w' } else { '-' });
    s.push(if perm & 0o001 != 0 { 'x' } else { '-' });
    s
}

/// Builds a `du -sb` command string for remote execution, properly escaped.
#[must_use]
pub fn build_du_command(path: &str) -> String {
    // Manual single-quote escaping: wrap in ' and replace ' with '\''
    let escaped = path.replace('\'', "'\\''");
    format!("du -sb '{escaped}' 2>/dev/null || echo 0")
}

#[cfg(test)]
mod tests;
