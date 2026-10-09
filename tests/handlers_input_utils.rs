use fm::handlers::input_utils;
use termina::event::{KeyCode, Modifiers};

#[test]
fn test_keyevent_to_string() {
    assert_eq!(
        input_utils::keyevent_to_string(KeyCode::Function(3), Modifiers::CONTROL),
        "Ctrl-F3"
    );
    assert_eq!(
        input_utils::keyevent_to_string(KeyCode::Char('p'), Modifiers::CONTROL),
        "Ctrl-p"
    );
    assert_eq!(
        input_utils::keyevent_to_string(KeyCode::Left, Modifiers::ALT),
        "Alt-Left"
    );
}

// ---------------------------------------------------------------------------
// Source guards for the single text-input driver.
//
// `input_utils::handle_text_input` is the only char-safe editor. Every editor
// it drives keeps `cursor_position` as a **char** index, while `String::insert`
// / `String::remove` take **byte** offsets.
//
// These tests enforce the
// invariant by scanning the tree instead of trusting code review. Each rule is
// deliberately narrow: a guard that fires on unrelated code gets disabled,
// which is worse than having no guard at all.
// ---------------------------------------------------------------------------

/// Every `.rs` file under `src/`.
fn source_files() -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    let mut out = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut out,
    );
    out.sort();
    assert!(!out.is_empty(), "no sources found under src/");
    out
}

/// `(relative path, line number, trimmed line)` for lines matching `predicate`.
fn scan(
    files: &[std::path::PathBuf],
    predicate: impl Fn(&str) -> bool,
) -> Vec<(String, usize, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut hits = Vec::new();
    for file in files {
        let Ok(src) = std::fs::read_to_string(file) else {
            continue;
        };
        for (idx, raw) in src.lines().enumerate() {
            let line = raw.trim();
            if predicate(line) {
                hits.push((
                    file.strip_prefix(root)
                        .unwrap_or(file)
                        .display()
                        .to_string(),
                    idx + 1,
                    line.to_string(),
                ));
            }
        }
    }
    hits
}

/// The receiver of a method call on `line`, e.g. `text` for `text.insert(..)`.
/// Scans back over whitespace so multi-line receivers still resolve.
fn receiver_before<'a>(line: &'a str, method: &str) -> Option<&'a str> {
    let start = line.find(method)?;
    line[..start].trim_end().rsplit(' ').next()
}

/// The only modules allowed to mutate a text buffer at a cursor position.
/// `clipboard_utils::insert_text_at_cursor_unicode` is the helper that
/// `handle_text_input` delegates to for pastes.
const CURSOR_EDIT_OWNERS: &[&str] = &[
    "src/handlers/input_utils.rs",
    "src/handlers/clipboard_utils.rs",
];

/// The byte-indexed `String` mutators. `HashMap::insert` / `Vec::remove` share
/// these names, so the receiver check below is what keeps this rule precise.
const STRING_MUTATORS: &[&str] = &[".insert_str(", ".insert(", ".remove("];

/// Popups must never hand-edit a text buffer with cursor math.
///
/// Any `text.insert(..)` / `text.remove(..)` outside the two owner modules is a
/// re-implementation of `handle_text_input` and a byte/char panic waiting for a
/// non-ASCII keystroke.
#[test]
fn test_only_the_text_input_driver_edits_a_string_at_a_cursor() {
    let files = source_files();
    let mut offenders = Vec::new();

    for method in STRING_MUTATORS {
        for (path, line, text) in scan(&files, |line| line.contains(method)) {
            let edits_text_buffer =
                receiver_before(&text, method) == Some("text") || text.contains("cursor_position");
            if edits_text_buffer && !CURSOR_EDIT_OWNERS.contains(&path.as_str()) {
                offenders.push(format!("{path}:{line}: {text}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "text buffer edited outside the text-input driver; route key handling \
         through input_utils::handle_text_input instead:\n{}",
        offenders.join("\n")
    );
}

/// Every `cursor_position = ...` initialiser has to be char-based.
///
/// This is the rule that would have caught review P1-4 (`rename_tab`) and the
/// same byte/char bug in the `rename` popup: both sized the cursor with `.len()`
/// (bytes) while the driver treats it as a char index.
#[test]
fn test_cursor_position_is_never_initialised_from_a_byte_length() {
    let files = source_files();
    let offenders: Vec<String> = scan(&files, |line| line.contains("cursor_position ="))
        .into_iter()
        .filter(|(_, _, text)| text.contains(".len()") && !text.contains("chars().count()"))
        .map(|(path, line, text)| format!("{path}:{line}: {text}"))
        .collect();

    assert!(
        offenders.is_empty(),
        "cursor_position is a char index; size it with `.chars().count()`, not \
         `.len()`:\n{}",
        offenders.join("\n")
    );
}

/// Both owner modules still exist and still own the char/byte conversion.
///
/// A rename that moves the driver would otherwise silently void the rule above.
#[test]
fn test_text_input_driver_still_owns_the_byte_conversion() {
    let files = source_files();
    let conversions: Vec<String> = scan(&files, |line| {
        line.contains("char_indices()") && (line.contains(".nth(") || line.contains("nth("))
    })
    .into_iter()
    .map(|(path, line, text)| format!("{path}:{line}: {text}"))
    .collect();

    assert!(
        conversions
            .iter()
            .any(|hit| hit.starts_with("src/handlers/input_utils.rs")),
        "input_utils::handle_text_input no longer maps char indices to byte \
         offsets; the text-input driver lost its char-safety boundary:\n{}",
        conversions.join("\n")
    );
}
