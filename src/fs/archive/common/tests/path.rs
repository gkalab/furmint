use super::*;

#[test]
fn normalizer_rejects_or_normalizes_safely() {
    assert!(normalize_relative("../evil").is_none());
    assert!(normalize_relative("../pwned").is_none());
    assert!(normalize_relative("sub/../../pwned2").is_none());
    assert!(normalize_relative("a/../../b").is_none());
    assert_eq!(normalize_relative("/abs").unwrap(), PathBuf::from("abs"));
    assert_eq!(normalize_relative("a/./b").unwrap(), PathBuf::from("a/b"));
    assert_eq!(normalize_relative("C:/x").unwrap(), PathBuf::from("x"));
    assert_eq!(normalize_relative("a/b/c").unwrap(), PathBuf::from("a/b/c"));
}

#[test]
fn resolve_symlinks_leaves_plain_paths_unchanged() {
    let symlinks = link_map(&[]);
    assert_eq!(
        resolve_symlinks(Path::new("a/b/c.txt"), &symlinks),
        PathBuf::from("a/b/c.txt")
    );
}

#[test]
fn resolve_symlinks_follows_relative_link_and_suffix() {
    let symlinks = link_map(&[("lib", "usr/lib")]);
    assert_eq!(
        resolve_symlinks(Path::new("lib"), &symlinks),
        PathBuf::from("usr/lib")
    );
    assert_eq!(
        resolve_symlinks(Path::new("lib/foo.so"), &symlinks),
        PathBuf::from("usr/lib/foo.so")
    );
}

#[test]
fn resolve_symlinks_treats_absolute_target_as_archive_root_relative() {
    let symlinks = link_map(&[("etc", "/usr/etc")]);
    assert_eq!(
        resolve_symlinks(Path::new("etc/hosts"), &symlinks),
        PathBuf::from("usr/etc/hosts")
    );
}

#[test]
fn resolve_symlinks_collapses_parent_components_in_target() {
    let symlinks = link_map(&[("a/link", "../b")]);
    assert_eq!(
        resolve_symlinks(Path::new("a/link/file"), &symlinks),
        PathBuf::from("b/file")
    );
}

#[test]
fn resolve_symlinks_follows_a_chain_of_links() {
    let symlinks = link_map(&[("a", "b"), ("b", "c")]);
    assert_eq!(
        resolve_symlinks(Path::new("a"), &symlinks),
        PathBuf::from("c")
    );
}

#[test]
fn resolve_symlinks_bounds_a_cycle() {
    // A cycle must not hang; the result is whatever the hop limit left.
    let symlinks = link_map(&[("a", "b"), ("b", "a")]);
    let resolved = resolve_symlinks(Path::new("a"), &symlinks);
    assert!(resolved == Path::new("a") || resolved == Path::new("b"));
}

#[test]
fn resolve_symlink_entries_marks_linked_directory_as_directory() {
    let mut entries = HashMap::from([
        (PathBuf::from("real"), entry("real", true, false, Some(0))),
        (PathBuf::from("link"), entry("link", false, true, Some(0))),
    ]);
    let symlinks = link_map(&[("link", "real")]);

    resolve_symlink_entries(&mut entries, &symlinks);

    let link = entries.get(Path::new("link")).unwrap();
    assert!(link.is_dir, "a linked directory must report as a directory");
    assert!(link.is_symlink, "the entry must keep its symlink flag");
    assert_eq!(link.size, None, "a linked directory has no stored size");
}

#[test]
fn resolve_symlink_entries_gives_linked_file_the_target_size() {
    let mut entries = HashMap::from([
        (
            PathBuf::from("real.txt"),
            entry("real.txt", false, false, Some(7)),
        ),
        (
            PathBuf::from("link.txt"),
            entry("link.txt", false, true, Some(0)),
        ),
    ]);
    let symlinks = link_map(&[("link.txt", "real.txt")]);

    resolve_symlink_entries(&mut entries, &symlinks);

    let link = entries.get(Path::new("link.txt")).unwrap();
    assert!(!link.is_dir);
    assert!(link.is_symlink);
    assert_eq!(link.size, Some(7));
}
