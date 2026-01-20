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
