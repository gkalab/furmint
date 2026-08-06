use fm::ui::ui_utils;
use std::path::Path;

#[test]
fn test_truncate_middle_with_ellipsis_short() {
    assert_eq!(
        ui_utils::truncate_middle_with_ellipsis("short.txt", 20),
        "short.txt"
    );
}

#[test]
fn test_truncate_middle_with_ellipsis_long() {
    let s = ui_utils::truncate_middle_with_ellipsis("verylongfilename.txt", 10);
    assert!(s.starts_with("very"));
    assert!(
        std::path::Path::new(&s)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
    );
    assert_eq!(s.chars().count(), 10);
}

#[test]
fn test_truncate_middle_with_ellipsis_unicode() {
    let s = ui_utils::truncate_middle_with_ellipsis("αβγδεζηθικλμνξο.txt", 10);
    assert_eq!(s.chars().count(), 10);
}

#[test]
fn test_truncate_path_with_ellipsis_long_path() {
    use std::path::PathBuf;
    let mut p = PathBuf::new();
    #[cfg(windows)]
    p.push("C:\\");
    #[cfg(not(windows))]
    p.push("/");

    p.push("home");
    p.push("user");
    p.push("projects");
    p.push("very");
    p.push("deep");
    p.push("path");

    let s = ui_utils::truncate_path_with_ellipsis(&p, 15);

    assert!(s.contains("…"));
    assert!(s.chars().count() <= 15);
    assert!(s.ends_with("path"));

    #[cfg(not(windows))]
    assert!(s.starts_with("/home"));
    #[cfg(windows)]
    assert!(s.starts_with("C:\\") && s.contains("…"));
}

#[test]
fn test_truncate_path_with_ellipsis_unicode() {
    let p = Path::new("/α/β/γ/δε/ζηθικλμνξο/ω");
    let s = ui_utils::truncate_path_with_ellipsis(p, 12);
    assert!(s.contains("…"));
    assert!(s.chars().count() <= 12);
}

#[test]
fn test_truncate_path_with_ellipsis_root() {
    let p = Path::new("/");
    let s = ui_utils::truncate_path_with_ellipsis(p, 5);
    assert_eq!(s, "/");
}

#[test]
fn test_replace_home_with_tilde_under_home() {
    let home = std::env::home_dir().expect("home dir should exist");
    let p = home.join("projects").join("deep");
    let s = ui_utils::replace_home_with_tilde(&p);
    let expected = ["~", "projects", "deep"]
        .iter()
        .collect::<std::path::PathBuf>();
    assert_eq!(std::path::Path::new(&s), expected);
}

#[test]
fn test_replace_home_with_tilde_exact_home() {
    let home = std::env::home_dir().expect("home dir should exist");
    let s = ui_utils::replace_home_with_tilde(&home);
    assert_eq!(s, "~");
}

#[test]
fn test_replace_home_with_tilde_outside_home() {
    let p = Path::new("/usr/share/doc");
    let s = ui_utils::replace_home_with_tilde(p);
    assert_eq!(s, "/usr/share/doc");
}

#[test]
fn test_replace_home_with_tilde_relative() {
    let p = Path::new("relative/path");
    let s = ui_utils::replace_home_with_tilde(p);
    assert_eq!(s, "relative/path");
}
