use super::*;
use std::fs;

#[cfg(unix)]
#[test]
fn test_symlink_to_dir_is_listed_as_dir() {
    use std::os::unix::fs::symlink;

    let base = std::env::temp_dir().join(format!("fm_test_symlink_dir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("target")).unwrap();
    std::fs::write(base.join("target/file.txt"), b"x").unwrap();
    symlink(base.join("target"), base.join("link_dir")).unwrap();
    symlink(base.join("target/file.txt"), base.join("link_file")).unwrap();
    symlink(base.join("missing"), base.join("link_broken")).unwrap();

    let entries = list_dir(&base).unwrap();
    let by_name: std::collections::HashMap<&str, &FileEntry> =
        entries.iter().map(|e| (e.name.as_str(), e)).collect();

    // A link to a directory must be enterable, so `is_dir` follows the link.
    let link_dir = by_name["link_dir"];
    assert!(link_dir.is_symlink);
    assert!(link_dir.is_dir);
    assert!(link_dir.size.is_none());
    assert!(link_dir.attributes.starts_with('l'));

    // A link to a file stays a file.
    let link_file = by_name["link_file"];
    assert!(link_file.is_symlink);
    assert!(!link_file.is_dir);
    // ... and is not runnable, even though `lrwxrwxrwx` sets every `x` bit.
    assert!(!is_executable(&base.join("link_file"), link_file));

    // A broken link keeps neither, so it stays openable as a file.
    let broken = by_name["link_broken"];
    assert!(broken.is_symlink);
    assert!(!broken.is_dir);

    // Directories (including linked ones) sort before plain files.
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["..", "link_dir", "target", "link_broken", "link_file"]
    );

    std::fs::remove_dir_all(&base).ok();
}

#[tokio::test]
async fn test_read_file_content() {
    use crate::fs::local::LocalFs;
    use crate::fs::provider::FileSystemProvider;
    use std::io::Write;
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("fm_test_read.txt");
    let mut file = std::fs::File::create(&test_file).unwrap();
    file.write_all(b"Hello, World!").unwrap();

    let provider = LocalFs::new();
    let result = provider.read_file_content(&test_file, 1024).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Hello, World!");

    std::fs::remove_file(&test_file).ok();
}

#[test]
fn test_list_dir_includes_parent() {
    let temp_dir = std::env::temp_dir();
    let result = list_dir(&temp_dir);
    assert!(result.is_ok());
    let entries = result.unwrap();
    assert_ne!(entries, Vec::new());
    assert_eq!(entries[0].name, "..");
    assert!(entries[0].is_dir);
}

#[test]
fn test_create_directory() {
    let temp_dir = std::env::temp_dir();
    let new_dir = temp_dir.join("fm_test_create_dir");
    if new_dir.exists() {
        fs::remove_dir_all(&new_dir).ok();
    }

    let result = create_directory(&new_dir);
    assert!(result.is_ok());
    assert!(new_dir.exists());

    let result_err = create_directory(&new_dir);
    assert!(result_err.is_err());

    fs::remove_dir_all(&new_dir).ok();
}

#[cfg(windows)]
#[test]
fn test_list_dir_follows_directory_link() {
    use std::io::Write;
    use std::os::windows::fs::symlink_dir;

    let target = tempfile::tempdir().unwrap();
    let file_path = target.path().join("linked_file.txt");
    {
        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(b"hello").unwrap();
    }

    let link_dir = tempfile::tempdir().unwrap();
    let link_path = link_dir.path().join("mylink");
    // Symlink creation needs Developer Mode or admin; skip otherwise.
    if symlink_dir(target.path(), &link_path).is_err() {
        return;
    }

    let entries = list_dir(&link_path).unwrap();
    assert!(
        entries.iter().any(|e| e.name == "linked_file.txt"),
        "should list the link target's contents, got: {entries:?}"
    );
}
