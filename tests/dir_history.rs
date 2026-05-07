use fm::dir_history::DirectoryHistory;
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn test_record_visit() {
    let mut history = DirectoryHistory {
        entries: HashMap::new(),
        cache_file: PathBuf::from("/tmp/test.json"),
    };

    let path = PathBuf::from("/home/user");
    history.record_visit("local", &path);
    assert_eq!(
        history
            .entries
            .get("local")
            .unwrap()
            .get(&path)
            .unwrap()
            .visit_count,
        1
    );

    history.record_visit("local", &path);
    assert_eq!(
        history
            .entries
            .get("local")
            .unwrap()
            .get(&path)
            .unwrap()
            .visit_count,
        2
    );
}

#[test]
fn test_fuzzy_search() {
    let mut history = DirectoryHistory {
        entries: HashMap::new(),
        cache_file: PathBuf::from("/tmp/test.json"),
    };

    history.record_visit("local", &PathBuf::from("/home/user"));
    history.record_visit("local", &PathBuf::from("/usr/local"));
    history.record_visit("local", &PathBuf::from("/var/log"));

    let results = history.fuzzy_search("local", "hm");
    assert!(!results.is_empty());
    assert!(
        results
            .iter()
            .any(|(p, _)| p.to_string_lossy().contains("home"))
    );
}

#[test]
fn test_fuzzy_search_sorting() {
    let mut history = DirectoryHistory {
        entries: HashMap::new(),
        cache_file: PathBuf::from("/tmp/test_fuzzy_sorting.json"),
    };

    let path_a = PathBuf::from("/home/user/documents");
    for _ in 0..10 {
        history.record_visit("local", &path_a);
    }

    let path_b = PathBuf::from("/home/user/downloads");
    history.record_visit("local", &path_b);

    let results = history.fuzzy_search("local", "do");

    let pos_a = results.iter().position(|(p, _)| p == &path_a);
    let pos_b = results.iter().position(|(p, _)| p == &path_b);

    assert!(pos_a.is_some(), "path_a should be in results");
    assert!(pos_b.is_some(), "path_b should be in results");

    assert!(
        pos_a.unwrap() < pos_b.unwrap(),
        "Highly visited path should come before less visited path"
    );
}

#[test]
fn test_history_limit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cache_file = temp_dir.path().join("dir_history.json");

    let mut history = DirectoryHistory {
        entries: HashMap::new(),
        cache_file: cache_file.clone(),
    };

    // Add 250 local entries
    for i in 0..250 {
        let path = PathBuf::from(format!("/local/path/{i}"));
        let visits = if i < 200 { 10 } else { 1 };
        for _ in 0..visits {
            history.record_visit("local", &path);
        }
    }

    // Add 100 remote entries
    for i in 0..100 {
        let path = PathBuf::from(format!("/remote/path/{i}"));
        let visits = if i < 50 { 10 } else { 1 };
        for _ in 0..visits {
            history.record_visit("[user@host]", &path);
        }
    }

    // Save and reload
    history.save().unwrap();

    let mut new_history = DirectoryHistory {
        entries: HashMap::new(),
        cache_file,
    };
    new_history.load().unwrap();

    // Check counts
    let local_entries = new_history.entries.get("local").unwrap();
    assert_eq!(
        local_entries.len(),
        DirectoryHistory::MAX_LOCAL_HISTORY_ENTRIES
    );

    let remote_entries = new_history.entries.get("[user@host]").unwrap();
    assert_eq!(
        remote_entries.len(),
        DirectoryHistory::MAX_REMOTE_HISTORY_ENTRIES
    );

    // Verify high-visit entries are kept
    for i in 0..200 {
        let path = PathBuf::from(format!("/local/path/{i}"));
        assert!(
            local_entries.contains_key(&path),
            "Local entry {i} should be kept"
        );
    }
    for i in 200..250 {
        let path = PathBuf::from(format!("/local/path/{i}"));
        assert!(
            !local_entries.contains_key(&path),
            "Local entry {i} should be discarded"
        );
    }

    for i in 0..50 {
        let path = PathBuf::from(format!("/remote/path/{i}"));
        assert!(
            remote_entries.contains_key(&path),
            "Remote entry {i} should be kept"
        );
    }
    for i in 50..100 {
        let path = PathBuf::from(format!("/remote/path/{i}"));
        assert!(
            !remote_entries.contains_key(&path),
            "Remote entry {i} should be discarded"
        );
    }
}
