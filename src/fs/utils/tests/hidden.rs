use super::*;

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
