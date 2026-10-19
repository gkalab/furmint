#![allow(clippy::cast_possible_truncation)]
use super::*;
use crate::fs::archive::ArchiveFormat;
use crate::tasks::UiEvent;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

fn test_progress(cancel: bool) -> TaskProgressContext {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let tx = crate::tasks::EventBus::new(tx);
    TaskProgressContext {
        id: 1,
        tx,
        cancel: Arc::new(AtomicBool::new(cancel)),
        processed_bytes: Arc::new(AtomicU64::new(0)),
        total_bytes: 0,
    }
}

/// Builds a raw RPM header structure (magic, version, reserved, count,
/// size, index entries, tag data) from explicit fields.
fn rpm_header(count: u32, size: u32, index: &[(i32, i32, i32, i32)], data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(16 + index.len() * 16 + data.len());
    v.extend_from_slice(&[0x8e, 0xad, 0xe8, 1]);
    v.extend_from_slice(&[0u8; 4]);
    v.extend_from_slice(&count.to_be_bytes());
    v.extend_from_slice(&size.to_be_bytes());
    for (tag, ty, offset, cnt) in index {
        v.extend_from_slice(&tag.to_be_bytes());
        v.extend_from_slice(&ty.to_be_bytes());
        v.extend_from_slice(&offset.to_be_bytes());
        v.extend_from_slice(&cnt.to_be_bytes());
    }
    v.extend_from_slice(data);
    v
}

fn parse_header(buf: &[u8]) -> Result<HashMap<i32, RpmValue>> {
    RpmHandler::parse_header_internal(&mut Cursor::new(buf))
}

fn valid_header() -> (Vec<u8>, Vec<u8>) {
    let mut data = Vec::new();
    data.extend_from_slice(b"test\0"); // tag 1000 STRING @0
    data.extend_from_slice(b"1.0\0"); // tag 1001 STRING @5
    data.extend_from_slice(&1i32.to_be_bytes()); // tag 1002 INT32 @9
    data.extend_from_slice(b"GPL\0MIT\0"); // tag 1004 STRING_ARRAY @13
    let index = [
        (1000, 6, 0, 1),
        (1001, 6, 5, 1),
        (1002, 4, 9, 1),
        (1004, 8, 13, 2),
    ];
    (
        rpm_header(index.len() as u32, data.len() as u32, &index, &data),
        data,
    )
}

#[test]
fn valid_header_parses_all_supported_types() {
    let (buf, _data) = valid_header();
    let tags = parse_header(&buf).unwrap();

    assert_eq!(tags.get(&1000), Some(&RpmValue::String("test".to_string())));
    assert_eq!(tags.get(&1001), Some(&RpmValue::String("1.0".to_string())));
    assert_eq!(tags.get(&1002), Some(&RpmValue::Int32(1)));
    assert_eq!(
        tags.get(&1004),
        Some(&RpmValue::StringArray(vec![
            "GPL".to_string(),
            "MIT".to_string()
        ]))
    );
}

#[test]
fn invalid_header_magic_is_error() {
    let (mut buf, _data) = valid_header();
    buf[0] = 0x00;
    let err = parse_header(&buf).unwrap_err().to_string();
    assert!(err.contains("magic"), "{err}");
}

#[test]
fn truncated_header_is_error() {
    let buf = rpm_header(0, 0, &[], &[]);
    assert!(parse_header(&buf[..10]).is_err());
}

#[test]
fn tag_count_above_limit_is_error() {
    // 10_001 index entries claimed, none present: must error before any
    // allocation proportional to the (untrusted) count.
    let buf = rpm_header(MAX_RPM_TAG_COUNT + 1, 0, &[], &[]);
    let err = parse_header(&buf).unwrap_err().to_string();
    assert!(err.contains("tag count"), "{err}");
}

#[test]
fn huge_data_size_is_error() {
    // Claims 2 GiB of tag data while providing none: the old code would
    // have allocated the whole thing.
    let buf = rpm_header(0, 0x8000_0000, &[], &[]);
    let err = parse_header(&buf).unwrap_err().to_string();
    assert!(err.contains("end of data"), "{err}");
}

#[test]
fn index_larger_than_stream_is_error() {
    let buf = rpm_header(100, 0, &[], &[]);
    assert!(parse_header(&buf).is_err());
}

#[test]
fn negative_tag_offset_is_error() {
    let data = b"x\0";
    let buf = rpm_header(1, data.len() as u32, &[(1000, 6, -1, 1)], data);
    let err = parse_header(&buf).unwrap_err().to_string();
    assert!(err.contains("offset"), "{err}");
}

#[test]
fn string_offset_out_of_bounds_is_error() {
    let data = b"x\0";
    let buf = rpm_header(1, data.len() as u32, &[(1000, 6, 50, 1)], data);
    let err = parse_header(&buf).unwrap_err().to_string();
    assert!(err.contains("out of bounds"), "{err}");
}

#[test]
fn int32_out_of_bounds_is_error() {
    let data = [0u8, 0];
    let buf = rpm_header(1, data.len() as u32, &[(1002, 4, 0, 1)], &data);
    assert!(parse_header(&buf).is_err());
}

#[test]
fn int32_array_overflow_is_error() {
    let data = [0u8; 16];
    // Claims 10 ints (40 bytes) inside a 16-byte data section.
    let buf = rpm_header(1, data.len() as u32, &[(1002, 4, 0, 10)], &data);
    assert!(parse_header(&buf).is_err());
}

#[test]
fn string_array_overflow_is_error() {
    let data = b"a\0b\0";
    // Asks for 5 NUL-terminated strings from a 4-byte section.
    let buf = rpm_header(1, data.len() as u32, &[(1004, 8, 0, 5)], data);
    assert!(parse_header(&buf).is_err());
}

fn build_cpio_payload() -> Vec<u8> {
    let entries = vec![
        (
            cpio::NewcBuilder::new("./usr").mode(0o040_755),
            Cursor::new(Vec::new()),
        ),
        (
            cpio::NewcBuilder::new("./usr/hello.txt").mode(0o100_644),
            Cursor::new(b"hello rpm".to_vec()),
        ),
    ];
    cpio::write_cpio(entries.into_iter(), Vec::new()).unwrap()
}

/// Minimal RPM: 96-byte lead, two empty headers, uncompressed cpio payload.
fn build_rpm(cpio_payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0u8; 96];
    for _ in 0..2 {
        v.extend_from_slice(&[0x8e, 0xad, 0xe8, 1]);
        v.extend_from_slice(&[0u8; 4]); // reserved
        v.extend_from_slice(&0u32.to_be_bytes()); // count
        v.extend_from_slice(&0u32.to_be_bytes()); // size
    }
    v.extend_from_slice(cpio_payload);
    v
}

fn write_rpm(tmp: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = tmp.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn rpm_scan_read_and_extract_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let rpm_path = write_rpm(tmp.path(), "test.rpm", &build_rpm(&build_cpio_payload()));
    let handler = RpmHandler::new(&rpm_path);

    let entries = handler.scan().unwrap().entries;
    let file = entries.get(&PathBuf::from("usr/hello.txt")).unwrap();
    assert_eq!(file.name, "hello.txt");
    assert!(!file.is_dir);
    assert_eq!(file.size, Some(b"hello rpm".len() as u64));
    let dir = entries.get(&PathBuf::from("usr")).unwrap();
    assert!(dir.is_dir);

    assert_eq!(handler.read_file("usr/hello.txt").unwrap(), b"hello rpm");

    let dest = tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();
    let progress = test_progress(false);
    handler.extract("", &dest, true, &progress).unwrap();
    assert_eq!(
        std::fs::read_to_string(dest.join("usr/hello.txt")).unwrap(),
        "hello rpm"
    );
}

#[test]
fn rpm_extract_stops_when_cancelled() {
    let tmp = tempfile::tempdir().unwrap();
    let rpm_path = write_rpm(tmp.path(), "test.rpm", &build_rpm(&build_cpio_payload()));
    let handler = RpmHandler::new(&rpm_path);

    let dest = tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();

    let progress = test_progress(true);
    handler.extract("", &dest, true, &progress).unwrap();

    let remaining: Vec<_> = std::fs::read_dir(&dest)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(remaining.is_empty(), "cancelled extract wrote entries");
}

#[test]
fn corrupt_rpm_metadata_is_error_not_panic() {
    let tmp = tempfile::tempdir().unwrap();

    let payloads: [&[u8]; 3] = [b"\x01\x02\x03\x04", b"", b"\x8e\xad\xe8\x01"];
    for bytes in payloads {
        let path = write_rpm(tmp.path(), "bad.rpm", bytes);
        let handler = RpmHandler::new(&path);
        assert!(
            handler.get_metadata().is_err(),
            "expected error for {bytes:?}"
        );
    }
}
