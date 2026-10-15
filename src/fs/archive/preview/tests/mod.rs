use super::*;

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
        FileEntry {
            name: "src".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    );

    entries.insert(
        file1.clone(),
        FileEntry {
            name: "main.rs".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(1024),
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    );

    entries.insert(
        file2.clone(),
        FileEntry {
            name: "README.md".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(512),
            modified: None,
            attributes: String::new(),
            selected: false,
        },
    );

    tree.insert(root, vec![dir1.clone(), file2.clone()]);
    tree.insert(dir1, vec![file1]);

    let rows = build_archive_tree(&ScanResult::new(entries, tree, HashMap::new()));

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
