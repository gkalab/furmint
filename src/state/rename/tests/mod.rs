use super::*;
use std::path::PathBuf;

#[test]
fn test_show_selects_the_stem_before_a_multibyte_extension() {
    let mut s = RenameState::new();
    s.show("日本語.txt".to_string(), PathBuf::from("/tmp"), false);
    assert_eq!(s.new_name.text(), "日本語.txt");
    // file_stem is "日本語" — 3 chars, 9 bytes.
    assert_eq!(s.new_name.cursor(), 3);
    assert!(s.new_name.cursor() <= s.new_name.text().chars().count());
    // The stem is selected, so the cursor is its end and the extension is not
    // part of the selection.
    assert_eq!(s.new_name.selection(), Some(0..3));
    assert_eq!(s.new_name.selected_text().as_deref(), Some("日本語"));
}

#[test]
fn test_show_selects_the_whole_name_without_extension() {
    let mut s = RenameState::new();
    s.show("héllo".to_string(), PathBuf::from("/tmp"), false);
    assert_eq!(s.new_name.cursor(), 5);
    assert_eq!(s.new_name.cursor(), s.new_name.text().chars().count());
    assert_eq!(s.new_name.selection(), Some(0..5));
}

#[test]
fn test_show_selects_before_ascii_extension() {
    let mut s = RenameState::new();
    s.show("archive.tar.gz".to_string(), PathBuf::from("/tmp"), false);
    // `file_stem` strips only the last extension: "archive.tar" is 11 chars.
    assert_eq!(s.new_name.cursor(), 11);
    assert_eq!(s.new_name.selection(), Some(0..11));
    assert_eq!(s.new_name.selected_text().as_deref(), Some("archive.tar"));
}

#[test]
fn test_show_selects_a_leading_dot_name_in_full() {
    let mut s = RenameState::new();
    // A dotfile has no extension, so the whole name is the stem.
    s.show(".gitignore".to_string(), PathBuf::from("/tmp"), false);
    assert_eq!(s.new_name.cursor(), 10);
    assert_eq!(s.new_name.selection(), Some(0..10));
}

#[test]
fn test_show_selects_the_stem_of_a_directory() {
    let mut s = RenameState::new();
    s.show("my.folder".to_string(), PathBuf::from("/tmp"), true);
    assert!(s.is_dir);
    assert_eq!(s.new_name.selection(), Some(0..2));
    assert_eq!(s.new_name.cursor(), 2);
}
