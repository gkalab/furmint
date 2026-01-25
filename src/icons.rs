//! Maps file types and extensions to nerd font icons
//! Returns the appropriate icon character for display in the file listing

/// Get the icon for a file or directory
///
/// # Arguments
/// * `name` - The file or directory name
/// * `is_dir` - Whether this is a directory
/// * `is_executable` - Whether this is an executable file
///
/// # Returns
/// A string containing the nerd font icon character
pub fn get_icon(name: &str, is_dir: bool, is_executable: bool) -> &'static str {
    // Directories get folder icon
    if is_dir {
        return "󰉋";
    }

    // Check for known filenames (case-insensitive)
    let name_lower = name.to_lowercase();
    match name_lower.as_str() {
        "license" | "license.txt" | "license.md" => return "",
        "readme" | "readme.md" | "readme.txt" => return "󰂺",
        "cargo.toml" | "cargo.lock" => return "",
        "package.json" | "package-lock.json" => return "",
        "dockerfile" => return "󰡨",
        "makefile" => return "",
        ".gitignore" | ".gitattributes" | ".gitmodules" => return "",
        ".dockerignore" => return "󰡨",
        _ => {}
    }

    // Check for file extension
    if let Some(ext_start) = name.rfind('.') {
        let ext = &name[ext_start + 1..].to_lowercase();
        match ext.as_str() {
            // Programming languages
            "rs" => return "",
            "py" => return "",
            "js" | "jsx" => return "",
            "ts" => return "",
            "tsx" => return "",
            "go" => return "",
            "java" => return "",
            "c" => return "",
            "cpp" | "cc" | "cxx" => return "",
            "h" | "hpp" => return "",
            "cs" => return "󰌛",
            "php" => return "",
            "rb" => return "",
            "swift" => return "",
            "kt" => return "󱈙",
            "scala" => return "",
            "r" => return "󰟔",
            "lua" => return "",
            "vim" => return "",
            "sh" | "bash" | "zsh" | "fish" => return "",

            // Web
            "html" | "htm" => return "",
            "css" | "scss" | "sass" | "less" => return "",
            "json" => return "",
            "xml" => return "󰗀",
            "yaml" | "yml" => return "",
            "toml" => return "",

            // Documents
            "md" | "markdown" => return "",
            "txt" => return "󰈙",
            "pdf" => return "",
            "doc" | "docx" => return "󰈬",
            "xls" | "xlsx" => return "󰈛",
            "ppt" | "pptx" => return "󰈧",

            // Images
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "svg" => return "",
            "webp" => return "",

            // Archives
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => return "",

            // Audio/Video
            "mp3" | "wav" | "flac" | "ogg" | "m4a" => return "",
            "mp4" | "avi" | "mkv" | "mov" | "webm" => return "",

            // Other
            "sql" => return "",
            "db" | "sqlite" | "sqlite3" => return "",
            "log" => return "󰌱",
            "lock" => return "",

            _ => {}
        }
    }

    // Executables get executable icon
    if is_executable {
        return "";
    }

    // Default file icon
    "󰈔"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directory_icon() {
        assert_eq!(get_icon("mydir", true, false), "󰉋");
    }

    #[test]
    fn test_executable_icon() {
        assert_eq!(get_icon("script", false, true), "");
    }

    #[test]
    fn test_rust_file_icon() {
        assert_eq!(get_icon("main.rs", false, false), "");
    }

    #[test]
    fn test_python_file_icon() {
        assert_eq!(get_icon("script.py", false, false), "");
    }

    #[test]
    fn test_javascript_file_icon() {
        assert_eq!(get_icon("app.js", false, false), "");
    }

    #[test]
    fn test_markdown_file_icon() {
        assert_eq!(get_icon("README.md", false, false), "󰂺");
    }

    #[test]
    fn test_known_filename_license() {
        assert_eq!(get_icon("LICENSE", false, false), "");
    }

    #[test]
    fn test_known_filename_readme() {
        assert_eq!(get_icon("README", false, false), "󰂺");
    }

    #[test]
    fn test_known_filename_cargo_toml() {
        assert_eq!(get_icon("Cargo.toml", false, false), "");
    }

    #[test]
    fn test_known_filename_package_json() {
        assert_eq!(get_icon("package.json", false, false), "");
    }

    #[test]
    fn test_default_file_icon() {
        assert_eq!(get_icon("unknown.xyz", false, false), "󰈔");
    }

    #[test]
    fn test_case_insensitive_extension() {
        assert_eq!(get_icon("Main.RS", false, false), "");
        assert_eq!(get_icon("Script.PY", false, false), "");
    }

    #[test]
    fn test_case_insensitive_filename() {
        assert_eq!(get_icon("LICENSE", false, false), "");
        assert_eq!(get_icon("license", false, false), "");
        assert_eq!(get_icon("License.txt", false, false), "");
    }

    #[test]
    fn test_executable_does_not_take_precedence() {
        // Executable flag takes precedence over extension
        assert_eq!(get_icon("script.py", false, true), "");
    }

    #[test]
    fn test_typescript_file_icon() {
        assert_eq!(get_icon("app.tsx", false, false), "");
    }

    #[test]
    fn test_image_file_icon() {
        assert_eq!(get_icon("photo.png", false, false), "");
    }

    #[test]
    fn test_archive_file_icon() {
        assert_eq!(get_icon("backup.zip", false, false), "");
    }

    #[test]
    fn test_pdf_file_icon() {
        assert_eq!(get_icon("document.pdf", false, false), "");
    }

    #[test]
    fn test_directory_takes_precedence() {
        // Directory flag takes precedence over everything
        assert_eq!(get_icon("Cargo.toml", true, false), "󰉋");
    }
}
