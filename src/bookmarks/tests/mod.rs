use super::*;
use tempfile::tempdir;

#[test]
fn test_bookmark_store_basic() -> Result<()> {
    let dir = tempdir()?;
    let file_path = dir.path().join("bookmarks.json");
    let mut store = BookmarkStore {
        entries: Vec::new(),
        path: file_path,
    };

    let p1 = PathBuf::from("/home/user/docs");
    let p2 = PathBuf::from("/var/log");

    assert!(store.add(p1.clone()).unwrap());
    assert!(!store.add(p1.clone()).unwrap()); // Duplicate
    assert!(store.add(p2.clone()).unwrap());
    assert_eq!(store.entries.len(), 2);

    assert!(store.contains(&p1));
    assert!(store.contains(&p2));

    let results = store.fuzzy_search("log");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], p2);

    assert!(store.remove(0).unwrap());
    assert_eq!(store.entries.len(), 1);
    assert!(!store.contains(&p1));

    Ok(())
}

#[test]
fn test_remove_by_path() -> Result<()> {
    let dir = tempdir()?;
    let file_path = dir.path().join("bookmarks.json");
    let mut store = BookmarkStore {
        entries: Vec::new(),
        path: file_path,
    };

    let p1 = PathBuf::from("/home/user/docs");
    let p2 = PathBuf::from("/var/log");
    store.add(p1.clone()).unwrap();
    store.add(p2.clone()).unwrap();

    assert!(store.remove_by_path(&p1, false).unwrap());
    assert_eq!(store.entries.len(), 1);
    assert!(!store.contains(&p1));
    assert!(store.contains(&p2));

    // Removing non-existent path returns false
    assert!(!store.remove_by_path(&p1, false).unwrap());
    assert!(
        !store
            .remove_by_path(&PathBuf::from("/does/not/exist"), false)
            .unwrap()
    );

    Ok(())
}

#[test]
fn test_remove_by_path_distinguishes_local_and_remote() -> Result<()> {
    let dir = tempdir()?;
    let file_path = dir.path().join("bookmarks.json");
    let mut store = BookmarkStore {
        entries: Vec::new(),
        path: file_path,
    };

    let path = PathBuf::from("/home/user/src");
    // Local and remote bookmarks share the same path.
    store.add(path.clone()).unwrap();
    store
        .add_with_ssh(
            path.clone(),
            Some("u".to_string()),
            Some("host".to_string()),
            None,
        )
        .unwrap();
    assert_eq!(store.entries.len(), 2);

    // Removing the local one must leave the remote one intact.
    assert!(store.remove_by_path(&path, false).unwrap());
    assert_eq!(store.entries.len(), 1);
    assert!(store.find_entry(&path, true).is_some());
    assert!(store.find_entry(&path, false).is_none());

    // And vice-versa.
    assert!(store.remove_by_path(&path, true).unwrap());
    assert_eq!(store.entries.len(), 0);

    Ok(())
}

#[test]
fn test_remote_and_local_distinct() -> Result<()> {
    let dir = tempdir()?;
    let file_path = dir.path().join("bookmarks.json");
    let mut store = BookmarkStore {
        entries: Vec::new(),
        path: file_path,
    };

    let path = PathBuf::from("/home/user/src");
    // Local bookmark
    assert!(store.add(path.clone()).unwrap());
    // Remote bookmark with the same path is distinct
    assert!(
        store
            .add_with_ssh(
                path.clone(),
                Some("someuser".to_string()),
                Some("host".to_string()),
                None,
            )
            .unwrap()
    );
    assert!(
        store
            .add_with_ssh(
                path.clone(),
                Some("otheruser".to_string()),
                Some("host".to_string()),
                Some(2222),
            )
            .unwrap()
    );
    assert_eq!(store.entries.len(), 3);

    // Dedup: same user/host/path rejected
    assert!(
        !store
            .add_with_ssh(
                path.clone(),
                Some("someuser".to_string()),
                Some("host".to_string()),
                None,
            )
            .unwrap()
    );
    assert_eq!(store.entries.len(), 3);

    Ok(())
}

#[test]
fn test_display_string() {
    let local = BookmarkEntry {
        path: PathBuf::from("/home/user/src"),
        ssh_user: None,
        ssh_host: None,
        ssh_port: None,
        is_archive: false,
    };
    assert_eq!(local.display_string(), "/home/user/src");

    let remote_default = BookmarkEntry {
        path: PathBuf::from("/home/user/src"),
        ssh_user: Some("someuser".to_string()),
        ssh_host: Some("host".to_string()),
        ssh_port: None,
        is_archive: false,
    };
    assert_eq!(
        remote_default.display_string(),
        "someuser@host:/home/user/src"
    );

    let remote_port = BookmarkEntry {
        path: PathBuf::from("/home/user/src"),
        ssh_user: Some("someuser".to_string()),
        ssh_host: Some("host".to_string()),
        ssh_port: Some(2222),
        is_archive: false,
    };
    assert_eq!(
        remote_port.display_string(),
        "someuser@host:2222:/home/user/src"
    );
}

#[test]
fn test_serde_roundtrip_backward_compatible() -> Result<()> {
    let dir = tempdir()?;
    let file_path = dir.path().join("bookmarks.json");
    let mut store = BookmarkStore {
        entries: Vec::new(),
        path: file_path.clone(),
    };

    store.add(PathBuf::from("/home/user/local")).unwrap();
    store
        .add_with_ssh(
            PathBuf::from("/var/remote"),
            Some("bob".to_string()),
            Some("example.com".to_string()),
            Some(22),
        )
        .unwrap();

    let content = serde_json::to_string_pretty(&store.entries)?;
    let reloaded: Vec<BookmarkEntry> = serde_json::from_str(&content)?;
    assert_eq!(reloaded.len(), 2);
    assert!(!reloaded[0].is_remote());
    assert!(reloaded[1].is_remote());
    assert_eq!(reloaded[1].ssh_user.as_deref(), Some("bob"));
    assert_eq!(reloaded[1].ssh_port, Some(22));

    // Local-only legacy entry has None fields
    let legacy = serde_json::from_str::<Vec<BookmarkEntry>>(r#"[{"path":"/legacy/path"}]"#)?;
    assert_eq!(legacy.len(), 1);
    assert!(!legacy[0].is_remote());
    assert_eq!(legacy[0].ssh_user, None);
    assert_eq!(legacy[0].ssh_host, None);
    assert_eq!(legacy[0].ssh_port, None);

    Ok(())
}

#[test]
fn test_fuzzy_search_matches_display() -> Result<()> {
    let dir = tempdir()?;
    let file_path = dir.path().join("bookmarks.json");
    let mut store = BookmarkStore {
        entries: Vec::new(),
        path: file_path,
    };

    store
        .add_with_ssh(
            PathBuf::from("/home/user/src"),
            Some("someuser".to_string()),
            Some("myserver".to_string()),
            None,
        )
        .unwrap();
    let results = store.fuzzy_search("myserver");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], PathBuf::from("/home/user/src"));

    let entries = store.fuzzy_search_entries("myserver");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].display_string(),
        "someuser@myserver:/home/user/src"
    );

    Ok(())
}
