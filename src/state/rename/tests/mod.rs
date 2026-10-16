use super::*;
use std::path::PathBuf;

#[test]
fn test_show_places_caret_before_multibyte_extension() {
    let mut s = RenameState::new();
    s.show("日本語.txt".to_string(), PathBuf::from("/tmp"), false);
    assert_eq!(s.new_name.text(), "日本語.txt");
    // file_stem is "日本語" — 3 chars, 9 bytes.
    assert_eq!(s.new_name.cursor(), 3);
    assert!(s.new_name.cursor() <= s.new_name.text().chars().count());
}

#[test]
fn test_show_places_caret_at_end_without_extension() {
    let mut s = RenameState::new();
    s.show("héllo".to_string(), PathBuf::from("/tmp"), false);
    assert_eq!(s.new_name.cursor(), 5);
    assert_eq!(s.new_name.cursor(), s.new_name.text().chars().count());
}

#[test]
fn test_show_sets_caret_before_ascii_extension() {
    let mut s = RenameState::new();
    s.show("archive.tar.gz".to_string(), PathBuf::from("/tmp"), false);
    // `file_stem` strips only the last extension: "archive.tar" is 11 chars.
    assert_eq!(s.new_name.cursor(), 11);
}
