use super::*;
use std::path::Path;

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
