use super::*;

#[test]
fn test_directory_icon() {
    assert_eq!(get_icon("mydir", true, false, false), "󰉋");
}

#[test]
fn test_executable_icon() {
    assert_eq!(get_icon("script", false, false, true), "");
}

#[test]
fn test_rust_file_icon() {
    assert_eq!(get_icon("main.rs", false, false, false), "");
}

#[test]
fn test_python_file_icon() {
    assert_eq!(get_icon("script.py", false, false, false), "");
}

#[test]
fn test_javascript_file_icon() {
    assert_eq!(get_icon("app.js", false, false, false), "");
}

#[test]
fn test_markdown_file_icon() {
    assert_eq!(get_icon("README.md", false, false, false), "󰂺");
}

#[test]
fn test_known_filename_license() {
    assert_eq!(get_icon("LICENSE", false, false, false), "");
}

#[test]
fn test_known_filename_readme() {
    assert_eq!(get_icon("README", false, false, false), "󰂺");
}

#[test]
fn test_known_filename_cargo_toml() {
    assert_eq!(get_icon("Cargo.toml", false, false, false), "");
}

#[test]
fn test_known_filename_package_json() {
    assert_eq!(get_icon("package.json", false, false, false), "");
}

#[test]
fn test_default_file_icon() {
    assert_eq!(get_icon("unknown.xyz", false, false, false), "󰈔");
}

#[test]
fn test_case_insensitive_extension() {
    assert_eq!(get_icon("Main.RS", false, false, false), "");
    assert_eq!(get_icon("Script.PY", false, false, false), "");
}

#[test]
fn test_case_insensitive_filename() {
    assert_eq!(get_icon("LICENSE", false, false, false), "");
    assert_eq!(get_icon("license", false, false, false), "");
    assert_eq!(get_icon("License.txt", false, false, false), "");
}

#[test]
fn test_executable_does_not_take_precedence() {
    // Executable flag takes precedence over extension
    assert_eq!(get_icon("script.py", false, false, true), "");
}

#[test]
fn test_typescript_file_icon() {
    assert_eq!(get_icon("app.tsx", false, false, false), "");
}

#[test]
fn test_image_file_icon() {
    assert_eq!(get_icon("photo.png", false, false, false), "");
}

#[test]
fn test_archive_file_icon() {
    assert_eq!(get_icon("backup.zip", false, false, false), "");
}

#[test]
fn test_pdf_file_icon() {
    assert_eq!(get_icon("document.pdf", false, false, false), "");
}

#[test]
fn test_directory_takes_precedence() {
    // Directory flag takes precedence over everything
    assert_eq!(get_icon("Cargo.toml", true, false, false), FOLDER_ICON);
}

#[test]
fn test_symlinked_directory_icon() {
    assert_eq!(get_icon("linked-dir", true, true, false), SYMLINK_DIR_ICON);
}

#[test]
fn test_symlinked_file_icon() {
    assert_eq!(
        get_icon("linked.txt", false, true, false),
        SYMLINK_FILE_ICON
    );
}

#[test]
fn test_symlink_takes_precedence_over_extension_and_dir() {
    // A link is recognizable regardless of what it points at, and the
    // `lrwxrwxrwx` permissions of a link must not mask it.
    assert_eq!(get_icon("main.rs", false, true, true), SYMLINK_FILE_ICON);
    assert_eq!(get_icon("Cargo.toml", true, true, true), SYMLINK_DIR_ICON);
    assert_eq!(get_icon("README", true, true, false), SYMLINK_DIR_ICON);
}

#[test]
fn test_symlink_icons_differ_from_plain_icons() {
    assert_ne!(
        get_icon("dir", true, true, false),
        get_icon("dir", true, false, false)
    );
    assert_ne!(
        get_icon("file", false, true, false),
        get_icon("file", false, false, false)
    );
}
