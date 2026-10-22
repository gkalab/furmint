use super::*;
use std::time::SystemTime;

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
