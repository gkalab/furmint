//! Test suite for `fs::archive_fs`.
mod bookmarks;
mod copy;
mod metadata;
mod open;
mod read;
mod symlinks;
mod write;

use crate::app::AppState;
use crate::app_state::tabs::TabManager;
use crate::bookmarks::BookmarkEntry;
use crate::fs::archive_fs::ArchiveFs;
use crate::fs::local::LocalFs;
use crate::fs::provider::FileSystemProvider;
use crate::fs::provider::TaskProgressContext;
use crate::fs::utils::FileEntry;
use crate::handlers::navigation::handle_enter;
use crate::handlers::popup_bookmark::{
    handle_bookmark_add, handle_bookmark_event, refresh_bookmark_list,
};
use crate::opener::FileOpener;
use crate::tasks::{AlertEvent, FsEvent, UiEvent};
use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use termina::event::{KeyCode, Modifiers};
use tokio::sync::mpsc;

/// Mock file opener that records if `open()` was called
struct MockOpener {
    called: AtomicBool,
}

impl MockOpener {
    fn new() -> Self {
        Self {
            called: AtomicBool::new(false),
        }
    }

    fn was_called(&self) -> bool {
        self.called.load(Ordering::SeqCst)
    }
}

impl FileOpener for MockOpener {
    fn open(&self, _path: &Path) -> anyhow::Result<()> {
        self.called.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn test_app(
    entries: Vec<FileEntry>,
    task_tx: mpsc::UnboundedSender<UiEvent>,
    opener: Arc<dyn FileOpener + Send + Sync>,
) -> AppState {
    let mut tab = crate::test_utils::create_test_tab();
    tab.entries = entries;
    tab.current_dir = std::path::PathBuf::from("/tmp");
    let left_tm = TabManager {
        tabs: vec![tab.clone()],
        active_tab_index: 0,
    };
    let right_tm = TabManager {
        tabs: vec![tab],
        active_tab_index: 0,
    };
    crate::test_utils::TestAppBuilder::new()
        .left(left_tm)
        .right(right_tm)
        .task_tx(task_tx)
        .opener(opener)
        .build()
}

fn create_file_entry(name: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        is_dir,
        is_symlink: false,
        size: Some(1024),
        modified: None,
        attributes: String::new(),
        selected: false,
    }
}

/// Builds an `AppState` whose active tab is browsing `archive_path` from the
/// inside (as if the user had pressed Enter on it).
fn archive_tab_app(archive_path: &Path, task_tx: mpsc::UnboundedSender<UiEvent>) -> AppState {
    let mut tab = crate::test_utils::create_test_tab();
    tab.provider = Arc::new(ArchiveFs::new(archive_path).unwrap());
    tab.current_dir = PathBuf::from("/");
    tab.custom_title = Some(
        archive_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    );
    crate::test_utils::TestAppBuilder::new()
        .left(TabManager {
            tabs: vec![tab.clone()],
            active_tab_index: 0,
        })
        .right(TabManager {
            tabs: vec![tab],
            active_tab_index: 0,
        })
        .task_tx(task_tx)
        .build()
}

/// Creates a gzip-compressed tar at `path` from explicit directories, regular
/// files and symbolic links, so tests can describe archives that contain links.
fn make_tar_gz_with_links(
    path: &Path,
    dirs: &[&str],
    files: &[(&str, &[u8])],
    links: &[(&str, &str)],
) {
    let file = File::create(path).unwrap();
    let enc = flate2::write::GzEncoder::new(file, Compression::default());
    let mut tar = tar::Builder::new(enc);

    for dir in dirs {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_mode(0o755);
        header.set_size(0);
        header.set_cksum();
        tar.append_data(&mut header, dir, &[] as &[u8]).unwrap();
    }

    for (name, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(data.len() as u64);
        tar.append_data(&mut header, name, *data).unwrap();
    }

    for (link, target) in links {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_mode(0o777);
        header.set_size(0);
        tar.append_link(&mut header, link, target).unwrap();
    }

    tar.finish().unwrap();
}

/// Runs one `recursive_op` copy from `src_fs` to `dest_fs`.
///
/// Returns the task's final item count so callers can check that progress
/// landed exactly on the total and not past it.
async fn recursive_copy(
    src_fs: &dyn FileSystemProvider,
    dest_fs: &dyn FileSystemProvider,
    src: &Path,
    dest: &Path,
    total: usize,
) -> Result<usize, String> {
    let (raw_tx, mut rx) = mpsc::unbounded_channel();
    let tx = crate::tasks::EventBus::new(raw_tx);
    let cancel = Arc::new(AtomicBool::new(false));
    let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let processed_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (_, decision_rx) = mpsc::channel(8);
    let decision_rx = Arc::new(tokio::sync::Mutex::new(decision_rx));
    let subtree_counts = crate::fs::ops::SubtreeCounts::new();

    let ctx = crate::fs::ops::RecursiveOpContext {
        transfer: crate::fs::ops::Transfer {
            src_fs,
            dest_fs,
            action: crate::state::CopyMoveAction::Copy,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total,
            total_bytes: 0,
            processed: &processed,
            processed_bytes: &processed_bytes,
            decision_rx: &decision_rx,
            subtree_counts: &subtree_counts,
        },
        src,
        dest,
    };
    let mut decision_state = crate::fs::ops::DecisionState::new();
    let res = crate::fs::ops::recursive_op(ctx, &mut decision_state).await;

    // Surface task errors reported through the event channel as test failures.
    while let Ok(ev) = rx.try_recv() {
        if let UiEvent::Alert(AlertEvent::TaskError { message, .. }) = ev {
            return Err(message);
        }
    }
    res.map_err(|e| e.to_string())?;
    Ok(processed.load(Ordering::Relaxed))
}

/// Creates an archive at `path` containing `files` (and any parent directories).
fn make_archive(path: &Path, files: &[(&str, &[u8])]) {
    let parent = path.parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    let file = File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

/// Creates a gzip-compressed tar archive at `path` containing `files`.
fn make_tar_gz(path: &Path, files: &[(&str, &[u8])]) {
    let parent = path.parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    let file = File::create(path).unwrap();
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for (name, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, *data).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap();
}
