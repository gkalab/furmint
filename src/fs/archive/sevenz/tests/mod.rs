//! Test suite for `fs::archive::sevenz`.
mod copy;
mod permissions;
mod scan;
mod write;

use crate::fs::archive_fs::ArchiveFs;
use crate::fs::local::LocalFs;
use crate::fs::provider::FileSystemProvider;
use sevenz_rust2::{ArchiveReader, ArchiveWriter, Password};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

fn make_progress() -> crate::fs::provider::TaskProgressContext {
    crate::fs::provider::TaskProgressContext {
        id: 1,
        tx: crate::tasks::EventBus::new(tokio::sync::mpsc::unbounded_channel().0),
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        processed_bytes: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total_bytes: 0,
    }
}

fn create_empty_7z(path: &Path) {
    let writer = ArchiveWriter::create(path).expect("create writer ok");
    writer.finish().expect("finish ok");
}

fn create_7z_with_file(path: &Path, name: &str, content: &[u8]) {
    let mut writer = ArchiveWriter::create(path).expect("create writer ok");
    let entry = sevenz_rust2::ArchiveEntry::new_file(name);
    writer
        .push_archive_entry(entry, Some(std::io::Cursor::new(content)))
        .expect("push entry ok");
    writer.finish().expect("finish ok");
}
