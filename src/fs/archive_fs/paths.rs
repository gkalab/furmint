//! Mapping between provider paths and the archive's internal path space.

use super::ArchiveFs;
use std::path::{Path, PathBuf};

impl ArchiveFs {
    /// Resolves a path against the archive's virtual tree, collapsing `.` and `..`
    /// components (e.g. `some_dir/..` becomes the archive root `"."`).
    ///
    /// # Panics
    ///
    /// This function never panics.
    #[must_use]
    pub(super) fn resolve_internal_path(path: &Path) -> PathBuf {
        let rel_path = if path.has_root() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        let p_str = rel_path.to_string_lossy().replace('\\', "/");
        let p_norm = p_str.trim_end_matches('/');

        let mut resolved: Vec<String> = Vec::new();
        for comp in Path::new(p_norm).components() {
            match comp {
                std::path::Component::CurDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_) => {}
                std::path::Component::ParentDir => {
                    resolved.pop();
                }
                std::path::Component::Normal(name) => {
                    resolved.push(name.to_string_lossy().to_string());
                }
            }
        }

        if resolved.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(resolved.join("/"))
        }
    }

    /// The archive-internal destination for `path`: relative to the root, forward
    /// slashes, no trailing slash. The root itself maps to an empty string.
    ///
    /// # Panics
    ///
    /// This function never panics.
    #[must_use]
    pub(super) fn archive_internal_dest(path: &Path) -> String {
        let resolved = Self::resolve_internal_path(path);
        if resolved == Path::new(".") {
            String::new()
        } else {
            resolved.to_string_lossy().to_string()
        }
    }

    /// Normalizes `path` and follows any symbolic links in it, yielding the
    /// archive-relative path of the real entry. A path with no links comes back
    /// unchanged; a broken link resolves to a path that simply is not present.
    ///
    /// # Panics
    ///
    /// This function never panics.
    #[must_use]
    pub(super) fn resolve_path(&self, path: &Path) -> PathBuf {
        let normalized = Self::resolve_internal_path(path);
        let symlinks = self
            .symlinks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if symlinks.is_empty() {
            return normalized;
        }
        crate::fs::archive::common::resolve_symlinks(&normalized, &symlinks)
    }

    /// Whether `path` is a symbolic link, without following it.
    pub(super) fn is_symlink_sync(&self, path: &Path) -> bool {
        let normalized = Self::resolve_internal_path(path);
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&normalized)
            .is_some_and(|e| e.is_symlink)
    }
}
