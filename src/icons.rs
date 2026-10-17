//! Maps file types and extensions to nerd font icons
//! Returns the appropriate icon character for display in the file listing

/// Folder icon (`nf-md-folder`, U+F024B)
const FOLDER_ICON: &str = "󰉋";
/// Folder with an arrow (`nf-md-folder_move`, U+F0252).
/// Marks a directory that is reached through a symbolic link.
const SYMLINK_DIR_ICON: &str = "󰉒";
/// File with a link badge (`nf-md-file_link`, U+F1177).
/// Marks a file that is reached through a symbolic link.
const SYMLINK_FILE_ICON: &str = "󱅷";

/// Get the icon for a file or directory
///
/// # Arguments
/// * `name` - The file or directory name
/// * `is_dir` - Whether this is a directory (a symlink pointing at a directory counts)
/// * `is_symlink` - Whether this entry is a symbolic link
/// * `is_executable` - Whether this is an executable file
///
/// # Returns
/// A string containing the nerd font icon character
#[must_use]
pub fn get_icon(name: &str, is_dir: bool, is_symlink: bool, is_executable: bool) -> &'static str {
    if is_symlink {
        return if is_dir {
            SYMLINK_DIR_ICON
        } else {
            SYMLINK_FILE_ICON
        };
    }

    // Directories get folder icon
    if is_dir {
        return FOLDER_ICON;
    }

    // Check for known filenames (case-insensitive)
    let name_lower = name.to_lowercase();
    match name_lower.as_str() {
        "license" | "license.txt" | "license.md" => return "",
        "readme" | "readme.md" | "readme.txt" => return "󰂺",
        "cargo.toml" | "cargo.lock" => return "",
        "package.json" | "package-lock.json" => return "",
        "dockerfile" | ".dockerignore" => return "󰡨",
        "makefile" => return "",
        ".gitignore" | ".gitattributes" | ".gitmodules" => return "",
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
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "svg" | "webp" => return "",

            // Archives
            "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" => return "",

            // Audio/Video
            "mp3" | "wav" | "flac" | "ogg" | "m4a" => return "",
            "mp4" | "avi" | "mkv" | "mov" | "webm" => return "",

            // Other
            "sql" | "db" | "sqlite" | "sqlite3" => return "",
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
mod tests;
