use crate::fs::archive::ScanResult;
use crate::fs::archive_fs::ArchiveEntry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveTreeRow {
    pub prefix: String,
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[must_use]
pub fn build_archive_tree(scan_result: &ScanResult) -> Vec<ArchiveTreeRow> {
    let (entries, tree) = scan_result;
    let mut rows = Vec::new();

    let root = if tree.contains_key(Path::new(".")) {
        PathBuf::from(".")
    } else if tree.contains_key(Path::new("")) {
        PathBuf::from("")
    } else {
        PathBuf::from(".")
    };

    build_subtree(&root, "", 0, entries, tree, &mut rows);
    rows
}

fn build_subtree(
    dir: &Path,
    prefix: &str,
    depth: usize,
    entries: &HashMap<PathBuf, ArchiveEntry>,
    tree: &HashMap<PathBuf, Vec<PathBuf>>,
    rows: &mut Vec<ArchiveTreeRow>,
) {
    let Some(children) = tree.get(dir) else {
        return;
    };

    let mut children = children.clone();
    children.sort_by(|a, b| {
        let a_is_dir = entries
            .get(a)
            .map_or_else(|| tree.contains_key(a), |e| e.file_entry.is_dir);
        let b_is_dir = entries
            .get(b)
            .map_or_else(|| tree.contains_key(b), |e| e.file_entry.is_dir);

        let a_name = a.file_name().unwrap_or_default().to_string_lossy();
        let b_name = b.file_name().unwrap_or_default().to_string_lossy();

        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a_name.to_lowercase().cmp(&b_name.to_lowercase()),
        }
    });

    let count = children.len();
    for (i, child) in children.iter().enumerate() {
        let (current_prefix, next_prefix) = if depth == 0 {
            (String::new(), String::new())
        } else {
            let is_last = i == count - 1;
            let connector = if is_last { "└─ " } else { "├─ " };
            let current = format!("{prefix}{connector}");
            let next = format!("{prefix}{}", if is_last { "   " } else { "│  " });
            (current, next)
        };

        let entry_opt = entries.get(child);
        let name = child
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let is_dir = entries
            .get(child)
            .map_or_else(|| tree.contains_key(child), |e| e.file_entry.is_dir);
        let size = entry_opt.and_then(|e| e.file_entry.size);

        rows.push(ArchiveTreeRow {
            prefix: current_prefix,
            name,
            is_dir,
            size,
        });

        if is_dir {
            build_subtree(child, &next_prefix, depth + 1, entries, tree, rows);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::utils::FileEntry;

    #[test]
    fn test_build_archive_tree() {
        let mut entries = HashMap::new();
        let mut tree = HashMap::new();

        let root = PathBuf::from(".");
        let dir1 = PathBuf::from("src");
        let file1 = PathBuf::from("src/main.rs");
        let file2 = PathBuf::from("README.md");

        entries.insert(
            dir1.clone(),
            ArchiveEntry {
                file_entry: FileEntry {
                    name: "src".to_string(),
                    is_dir: true,
                    is_symlink: false,
                    size: None,
                    modified: None,
                    attributes: String::new(),
                    selected: false,
                },
                position: None,
            },
        );

        entries.insert(
            file1.clone(),
            ArchiveEntry {
                file_entry: FileEntry {
                    name: "main.rs".to_string(),
                    is_dir: false,
                    is_symlink: false,
                    size: Some(1024),
                    modified: None,
                    attributes: String::new(),
                    selected: false,
                },
                position: None,
            },
        );

        entries.insert(
            file2.clone(),
            ArchiveEntry {
                file_entry: FileEntry {
                    name: "README.md".to_string(),
                    is_dir: false,
                    is_symlink: false,
                    size: Some(512),
                    modified: None,
                    attributes: String::new(),
                    selected: false,
                },
                position: None,
            },
        );

        tree.insert(root, vec![dir1.clone(), file2.clone()]);
        tree.insert(dir1, vec![file1]);

        let rows = build_archive_tree(&(entries, tree));

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "src");
        assert!(rows[0].is_dir);
        assert_eq!(rows[0].prefix, "");

        assert_eq!(rows[1].name, "main.rs");
        assert!(!rows[1].is_dir);
        assert_eq!(rows[1].prefix, "└─ ");

        assert_eq!(rows[2].name, "README.md");
        assert!(!rows[2].is_dir);
        assert_eq!(rows[2].prefix, "");
    }
}
