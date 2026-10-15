use super::*;

#[test]
fn test_format_size_bytes() {
    assert_eq!(format_size(Some(500), false, false), "    500");
    assert_eq!(format_size(Some(0), false, false), "      0");
    assert_eq!(format_size(Some(999), false, false), "    999");
}

#[test]
fn test_format_size_kilobytes() {
    assert_eq!(format_size(Some(1024), false, false), "   1.0K");
    assert_eq!(format_size(Some(1536), false, false), "   1.5K");
}

#[test]
fn test_format_size_megabytes() {
    assert_eq!(format_size(Some(1_048_576), false, false), "   1.0M");
    assert_eq!(format_size(Some(1_572_864), false, false), "   1.5M");
}

#[test]
fn test_format_size_gigabytes() {
    assert_eq!(format_size(Some(1_073_741_824), false, false), "   1.0G");
    assert_eq!(format_size(Some(2_684_354_560), false, false), "   2.5G");
}

#[test]
fn test_format_size_dir() {
    assert_eq!(format_size(None, true, false), "  <DIR>");
    assert_eq!(format_size(Some(100), true, false), "  <DIR>");
}

#[test]
fn test_format_size_symlink() {
    assert_eq!(format_size(None, true, true), "  <LNK>");
}

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

#[test]
fn test_format_modified_some() {
    let now = SystemTime::now();
    let result = format_modified(Some(now));
    assert_eq!(result.len(), 19);
    assert!(result.contains('-'));
    assert!(result.contains(':'));
}

#[test]
fn test_format_modified_none() {
    assert_eq!(format_modified(None), "                   "); // 19 spaces for UI alignment
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
fn test_get_attributes_unix() {
    #[cfg(unix)]
    {
        let temp_dir = std::env::temp_dir();
        let meta = std::fs::metadata(&temp_dir).unwrap();
        let attrs = get_attributes(&meta, true, false);
        assert_eq!(attrs.chars().next().unwrap(), 'd');
        assert_eq!(attrs.len(), 10);
    }
}

#[test]
fn test_get_attributes_windows() {
    #[cfg(windows)]
    {
        let temp_dir = std::env::temp_dir();
        let meta = std::fs::metadata(&temp_dir).unwrap();
        let attrs = get_attributes(&meta, true, false);
        assert_eq!(attrs.trim_start().chars().next().unwrap(), 'd');
        assert_eq!(attrs.len(), 6);

        let temp_file = temp_dir.join("fm_test_attrs.txt");
        std::fs::write(&temp_file, b"test").unwrap();
        let meta = std::fs::metadata(&temp_file).unwrap();
        let attrs = get_attributes(&meta, false, false);
        assert_eq!(attrs.trim_start().chars().next().unwrap(), '-');
        assert_eq!(attrs.len(), 6);
        std::fs::remove_file(&temp_file).ok();
    }
}

#[test]
fn test_is_executable() {
    #[cfg(unix)]
    {
        use std::fs::{self, File};
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("fm_test_exe");
        {
            let _ = File::create(&file_path).unwrap();
        }
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&file_path, perms).unwrap();

        let entry = FileEntry {
            name: "fm_test_exe".to_string(),
            is_dir: false,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: String::new(),
            selected: false,
        };

        assert!(is_executable(&file_path, &entry));

        perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&file_path, perms).unwrap();
        assert!(!is_executable(&file_path, &entry));

        fs::remove_file(&file_path).ok();
    }
}

#[test]
fn test_is_executable_from_attributes() {
    #[cfg(unix)]
    {
        // Test executable detection from attributes field (for remote files)
        let executable_entry = FileEntry {
            name: "remote_script".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(1024),
            modified: None,
            attributes: "-rwxr-xr-x".to_string(),
            selected: false,
        };

        // Use a non-existent path to force fallback to attributes parsing
        let fake_path = std::path::PathBuf::from("/nonexistent/remote_script");
        assert!(is_executable(&fake_path, &executable_entry));

        // Test non-executable file
        let non_executable_entry = FileEntry {
            name: "remote_file".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(1024),
            modified: None,
            attributes: "-rw-r--r--".to_string(),
            selected: false,
        };

        assert!(!is_executable(&fake_path, &non_executable_entry));

        // Test directory (should not be executable even with x bits)
        let dir_entry = FileEntry {
            name: "remote_dir".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: "drwxr-xr-x".to_string(),
            selected: false,
        };

        assert!(!is_executable(&fake_path, &dir_entry));

        // Test file with only user execute permission
        let user_exec_entry = FileEntry {
            name: "user_exec".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(512),
            modified: None,
            attributes: "-rwx------".to_string(),
            selected: false,
        };

        assert!(is_executable(&fake_path, &user_exec_entry));
    }

    #[cfg(windows)]
    {
        // Test executable detection by file extension on Windows
        let some_entry = FileEntry {
            name: "some_name".to_string(),
            is_dir: false,
            is_symlink: false,
            size: Some(1024),
            modified: None,
            attributes: "<FILE>".to_string(),
            selected: false,
        };

        let fake_path = std::path::PathBuf::from("C:\\nonexistent\\program.exe");
        assert!(is_executable(&fake_path, &some_entry));

        let fake_path = std::path::PathBuf::from("C:\\nonexistent\\script.bat");
        assert!(is_executable(&fake_path, &some_entry));

        let fake_path = std::path::PathBuf::from("C:\\nonexistent\\command.com");
        assert!(is_executable(&fake_path, &some_entry));

        let fake_path = std::path::PathBuf::from("C:\\nonexistent\\script.cmd");
        assert!(is_executable(&fake_path, &some_entry));

        let fake_path = std::path::PathBuf::from("C:\\nonexistent\\document.txt");
        assert!(!is_executable(&fake_path, &some_entry));

        let dir_entry = FileEntry {
            name: "folder".to_string(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
            attributes: "<DIR>".to_string(),
            selected: false,
        };
        assert!(!is_executable(&fake_path, &dir_entry));
    }
}

#[test]
fn test_unix_attributes_allow_execute() {
    // The 10-character Unix format is recognized on every platform.
    assert_eq!(unix_attributes_allow_execute("-rwxr-xr-x"), Some(true));
    assert_eq!(unix_attributes_allow_execute("-rwx------"), Some(true));
    assert_eq!(unix_attributes_allow_execute("-rw-r-xr-x"), Some(true));
    assert_eq!(unix_attributes_allow_execute("-rw-r--r--"), Some(false));
    // Windows attribute flags and empty strings carry no Unix permissions.
    assert_eq!(unix_attributes_allow_execute("---r--"), None);
    assert_eq!(unix_attributes_allow_execute("d---r-"), None);
    assert_eq!(unix_attributes_allow_execute("<FILE>"), None);
    assert_eq!(unix_attributes_allow_execute(""), None);
}

#[test]
fn test_is_executable_from_unix_attributes_on_every_platform() {
    // Remote and archive entries report Unix permissions; the `x` bits
    // decide, not the name, also when running on Windows.
    let file = |name: &str, attributes: &str| FileEntry {
        name: name.to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(1024),
        modified: None,
        attributes: attributes.to_string(),
        selected: false,
    };

    let remote_path = std::path::PathBuf::from("/usr/local/bin/deploy");
    assert!(is_executable(&remote_path, &file("deploy", "-rwxr-xr-x")));
    // No execute bit anywhere, despite a Windows-ish extension, and the
    // name is irrelevant for entries that report Unix permissions.
    assert!(!is_executable(
        &remote_path,
        &file("deploy.exe", "-rw-r--r--")
    ));
    // Symlinked directories are never executable for display purposes.
    assert!(!is_executable(
        &remote_path,
        &FileEntry {
            is_dir: true,
            ..file("link", "lrwxrwxrwx")
        }
    ));
    // Neither are symlinked files: `lrwxrwxrwx` describes the link, not the
    // target, so Enter must not launch them in a terminal.
    assert!(!is_executable(
        &remote_path,
        &FileEntry {
            is_symlink: true,
            ..file("notes.txt", "lrwxrwxrwx")
        }
    ));
    // The target's own execute bit is irrelevant to the link's display.
    assert!(!is_executable(
        &remote_path,
        &FileEntry {
            is_symlink: true,
            ..file("deploy", "lrwxrwxrwx")
        }
    ));
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

#[test]
fn test_is_gui_executable_mock() {
    use std::io::Write;
    let temp_dir = std::env::temp_dir();
    let gui_exe = temp_dir.join("test_gui.exe");
    let cli_exe = temp_dir.join("test_cli.exe");
    let invalid_exe = temp_dir.join("test_invalid.exe");

    let create_pe = |path: &std::path::Path, subsystem: u16| {
        let mut file = std::fs::File::create(path).unwrap();
        let mut buf = vec![0u8; 1024];
        buf[0..2].copy_from_slice(b"MZ");
        // PE offset at 0x3C
        let pe_offset: u32 = 0x80;
        buf[60..64].copy_from_slice(&pe_offset.to_le_bytes());

        let pe_start = pe_offset as usize;
        buf[pe_start..pe_start + 4].copy_from_slice(b"PE\0\0");

        // Subsystem at pe_start + 4 + 20 + 68 = pe_start + 92
        let subsystem_offset = pe_start + 92;
        buf[subsystem_offset..subsystem_offset + 2].copy_from_slice(&subsystem.to_le_bytes());

        file.write_all(&buf).unwrap();
    };

    create_pe(&gui_exe, 2); // GUI
    create_pe(&cli_exe, 3); // CUI/Console

    {
        let mut file = std::fs::File::create(&invalid_exe).unwrap();
        file.write_all(b"not an exe").unwrap();
    }

    assert!(is_gui_executable(&gui_exe));
    assert!(!is_gui_executable(&cli_exe));
    assert!(!is_gui_executable(&invalid_exe));

    let _ = std::fs::remove_file(gui_exe);
    let _ = std::fs::remove_file(cli_exe);
    let _ = std::fs::remove_file(invalid_exe);
}

#[test]
fn test_normalize_sftp_path() {
    assert_eq!(normalize_sftp_path(Path::new("foo")), "/foo");
    assert_eq!(normalize_sftp_path(Path::new("/foo/bar/")), "/foo/bar");
    assert_eq!(
        normalize_sftp_path(Path::new("C:\\foo\\bar")),
        "/C:/foo/bar"
    );
    assert_eq!(normalize_sftp_path(Path::new("//foo///bar")), "/foo/bar");
    assert_eq!(normalize_sftp_path(Path::new("/")), "/");
}

#[test]
fn test_unix_conventions_follow_the_filesystem_not_the_host() {
    // Local on Windows is the only DOS-attribute filesystem; remote and
    // archive entries always report Unix modes, on any host.
    assert_eq!(has_unix_attributes(true), !cfg!(windows));
    assert!(has_unix_attributes(false));
}

#[test]
fn test_hidden_name_follows_the_unix_convention_of_the_filesystem() {
    assert!(!has_hidden_name("env", true));
    assert!(!has_hidden_name(".", true));
    assert!(!has_hidden_name("..", true));

    // The leading dot is the only Unix marker; a filesystem without the
    // convention has no hidden names at all.
    assert!(has_hidden_name(".env", true));
    assert!(has_hidden_name(".git", true));
    assert!(!has_hidden_name(".env", false));
    assert!(!has_hidden_name(".git", false));
}

#[test]
fn test_is_hidden_name_ignores_navigation_entries() {
    // `.` and `..` are navigation entries, not hidden ones, on any
    // filesystem.
    assert!(!has_hidden_name(".", true));
    assert!(!has_hidden_name("..", true));
    assert!(!has_hidden_name(".", false));
    assert!(!has_hidden_name("..", false));
}

#[test]
fn test_has_hidden_attribute_reads_the_windows_flag() {
    // A Windows attribute string is a type char plus the r/h/s/i/p flags,
    // so `h` at index 2 is FILE_ATTRIBUTE_HIDDEN.
    assert!(has_hidden_attribute("d-h---"));
    assert!(has_hidden_attribute("--h---"));
    assert!(has_hidden_attribute("drh---"));
    assert!(!has_hidden_attribute("d----"));
    assert!(!has_hidden_attribute("----h-"));

    // Unix permission strings put a permission char at index 2 and never an
    // `h`, so they are never mistaken for hidden.
    for attrs in ["drwxr-xr-x ", "-rwxr-xr-x", "lrwxrwxrwx ", "-rw-r--r--"] {
        assert!(!has_hidden_attribute(attrs), "{attrs:?} read as hidden");
    }

    // Empty or short strings simply carry no flags.
    assert!(!has_hidden_attribute(""));
    assert!(!has_hidden_attribute("d-"));
}

#[test]
fn test_is_hidden_accepts_either_signal() {
    // Windows marks hidden files with the DOS flag alone, dot or no dot.
    assert!(is_hidden("env", "d-h---", false));
    assert!(is_hidden("AppData", "d-h---", false));
    assert!(is_hidden("config.ini", "--h---", false));
    assert!(is_hidden(".env", "--h---", false));

    // The DOS flag also hides a dot-prefixed entry on a Unix filesystem.
    assert!(is_hidden(".env", "--h---", true));

    // The leading dot is only a marker on Unix-like filesystems.
    assert!(is_hidden(".env", "d----", true));
    assert!(is_hidden(".env", "", true));
    assert!(!is_hidden(".env", "d----", false));
    assert!(!is_hidden(".env", "", false));

    // Ordinary entries stay visible.
    assert!(!is_hidden("env", "------", false));
    assert!(!is_hidden("env", "--s---", false));
    assert!(!is_hidden("Documents", "d----", false));
    assert!(!is_hidden(".", "d----", true));
    assert!(!is_hidden("..", "d----", true));
}

#[test]
fn test_remote_dotfiles_are_hidden_even_from_a_windows_client() {
    // The regression: a remote listing carries Unix permissions and has no
    // DOS flags at all, so a `cfg!(windows)` gate made every dotfile look
    // visible and left the hidden-file toggle with nothing to hide.
    for name in [".env", ".git", ".config", ".bashrc"] {
        assert!(
            is_hidden(name, format_sftp_permissions(0o100_644).as_str(), true),
            "{name:?} should be hidden in a remote listing"
        );
    }
    assert!(!is_hidden("README.md", "-rw-r--r--", true));
    assert!(!is_hidden("env", "-rw-r--r--", true));
}

#[cfg(windows)]
#[test]
fn test_windows_dot_names_are_not_hidden_locally() {
    // Windows Explorer shows `.git` like any other directory, so a leading
    // dot must not mute a *local* entry.
    for name in [".env", ".git", ".config"] {
        assert!(
            !has_hidden_name(name, false),
            "{name:?} should not be a hidden name on a local Windows filesystem"
        );
        assert!(
            !is_hidden(name, "d----", false),
            "{name:?} should not be hidden on Windows without the DOS flag"
        );
    }

    // The DOS flag still hides a dot-prefixed entry.
    assert!(is_hidden(".config", "d-h---", false));
}

#[test]
fn test_build_du_command() {
    assert_eq!(
        build_du_command("/path/with spaces"),
        "du -sb '/path/with spaces' 2>/dev/null || echo 0"
    );
    assert_eq!(
        build_du_command("/path'with'quotes"),
        "du -sb '/path'\\''with'\\''quotes' 2>/dev/null || echo 0"
    );
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
