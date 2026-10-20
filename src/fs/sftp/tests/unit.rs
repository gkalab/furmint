//! Unit tests for the SFTP backend.
//!
//! These drive the real client-side listing logic ([`build_dir_listing`]) over
//! the wire against an in-process SFTP server (`russh_sftp::server`) that is
//! backed by an actual local directory. No SSH transport is needed.

use super::super::transfer::build_dir_listing;
use crate::fs::utils::FileEntry;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{Attrs, File, FileAttributes, Handle, Name, Status, StatusCode};
use russh_sftp::server::{Handler, StatusReply};
use std::collections::HashMap;
use std::future::Future;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Minimal server error: just carries an SFTP status code.
#[derive(Debug, Clone, Copy)]
struct TestError(StatusCode);

impl From<TestError> for StatusReply {
    fn from(error: TestError) -> Self {
        error.0.into()
    }
}

/// Per-handle state for an open directory on the in-process server.
#[derive(Debug)]
struct TestDir {
    path: PathBuf,
    listed: bool,
}

/// In-process SFTP server rooted at a real local directory.
///
/// `opendir`/`readdir`/`close` keep per-handle state; `stat` follows
/// symlinks, `lstat` does not. Paths are sanitized to stay under the root.
#[derive(Debug)]
struct TestSftpd {
    root: PathBuf,
    next_handle: u64,
    dirs: HashMap<String, TestDir>,
}

impl TestSftpd {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            next_handle: 0,
            dirs: HashMap::new(),
        }
    }

    /// Joins a remote (POSIX) path under the root, refusing to escape it.
    fn resolve(&self, remote: &str) -> Result<PathBuf, TestError> {
        let mut path = self.root.clone();
        for component in remote.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                return Err(TestError(StatusCode::BadMessage));
            }
            path = path.join(component);
        }
        Ok(path)
    }

    fn opendir_sync(&mut self, id: u32, path: &str) -> Result<Handle, TestError> {
        let dir = self.resolve(path)?;
        std::fs::read_dir(&dir).map_err(|_| TestError(StatusCode::NoSuchFile))?;
        let handle = self.next_handle.to_string();
        self.next_handle += 1;
        self.dirs.insert(
            handle.clone(),
            TestDir {
                path: dir,
                listed: false,
            },
        );
        Ok(Handle { id, handle })
    }

    fn readdir_sync(&mut self, id: u32, handle: &str) -> Result<Name, TestError> {
        let dir = self
            .dirs
            .get_mut(handle)
            .ok_or(TestError(StatusCode::BadMessage))?;
        if dir.listed {
            // Signal end-of-directory; the client breaks on `Eof`.
            return Err(TestError(StatusCode::Eof));
        }
        dir.listed = true;
        let read = std::fs::read_dir(&dir.path).map_err(|_| TestError(StatusCode::Failure))?;
        let mut files = Vec::new();
        for entry in read {
            let entry = entry.map_err(|_| TestError(StatusCode::Failure))?;
            let name = entry.file_name().to_string_lossy().to_string();
            // `DirEntry::metadata` is lstat: a link reports its *own* mode,
            // which is exactly what READDIR must carry.
            let meta = entry
                .metadata()
                .map_err(|_| TestError(StatusCode::Failure))?;
            files.push(File::new(name, file_attrs(&meta)));
        }
        Ok(Name { id, files })
    }

    fn close_sync(&mut self, id: u32, handle: &str) -> Status {
        self.dirs.remove(handle);
        ok_status(id)
    }

    fn stat_sync(&self, id: u32, path: &str) -> Result<Attrs, TestError> {
        let meta = std::fs::metadata(self.resolve(path)?).map_err(|e| io_error(&e))?;
        Ok(Attrs {
            id,
            attrs: file_attrs(&meta),
        })
    }

    fn lstat_sync(&self, id: u32, path: &str) -> Result<Attrs, TestError> {
        let meta = std::fs::symlink_metadata(self.resolve(path)?).map_err(|e| io_error(&e))?;
        Ok(Attrs {
            id,
            attrs: file_attrs(&meta),
        })
    }
}

fn ok_status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: String::new(),
    }
}

fn io_error(error: &std::io::Error) -> TestError {
    if error.kind() == std::io::ErrorKind::NotFound {
        TestError(StatusCode::NoSuchFile)
    } else {
        TestError(StatusCode::Failure)
    }
}

/// Maps local metadata to SFTP attributes, keeping the full mode (type +
/// permission bits) so link/dir/file types survive the wire.
#[allow(clippy::cast_possible_truncation)]
fn file_attrs(meta: &std::fs::Metadata) -> FileAttributes {
    let unix = |st: Option<std::time::SystemTime>| {
        st.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32)
    };
    FileAttributes {
        size: Some(meta.len()),
        uid: Some(meta.uid()),
        gid: Some(meta.gid()),
        user: None,
        group: None,
        permissions: Some(meta.mode()),
        atime: unix(meta.accessed().ok()),
        mtime: unix(meta.modified().ok()),
    }
}

impl Handler for TestSftpd {
    type Error = TestError;

    fn unimplemented(&self) -> Self::Error {
        TestError(StatusCode::OpUnsupported)
    }

    fn opendir(
        &mut self,
        id: u32,
        path: String,
    ) -> impl Future<Output = Result<Handle, Self::Error>> + Send {
        let result = self.opendir_sync(id, &path);
        async move { result }
    }

    fn readdir(
        &mut self,
        id: u32,
        handle: String,
    ) -> impl Future<Output = Result<Name, Self::Error>> + Send {
        let result = self.readdir_sync(id, &handle);
        async move { result }
    }

    fn close(
        &mut self,
        id: u32,
        handle: String,
    ) -> impl Future<Output = Result<Status, Self::Error>> + Send {
        let result = self.close_sync(id, &handle);
        async move { Ok(result) }
    }

    fn stat(
        &mut self,
        id: u32,
        path: String,
    ) -> impl Future<Output = Result<Attrs, Self::Error>> + Send {
        let result = self.stat_sync(id, &path);
        async move { result }
    }

    fn lstat(
        &mut self,
        id: u32,
        path: String,
    ) -> impl Future<Output = Result<Attrs, Self::Error>> + Send {
        let result = self.lstat_sync(id, &path);
        async move { result }
    }
}

/// Spins up an in-process SFTP server over a duplex pair rooted at `dir` and
/// returns a ready-to-use session on the client end.
async fn spawn_server(dir: PathBuf) -> SftpSession {
    let handler = TestSftpd::new(dir);
    let (client, server) = tokio::io::duplex(65536);
    // `run` spawns the server loop and returns immediately.
    russh_sftp::server::run(server, handler).await;
    SftpSession::new(client).await.expect("SFTP handshake")
}

#[tokio::test]
async fn test_list_dir_resolves_remote_symlinks() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().to_path_buf();

    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("file.txt"), b"file").unwrap();
    std::fs::write(root.join("plain.txt"), b"plain").unwrap();
    symlink(&target, root.join("link_dir")).unwrap();
    symlink(root.join("plain.txt"), root.join("link_file")).unwrap();
    symlink(root.join("missing"), root.join("link_broken")).unwrap();

    let sftp = Arc::new(spawn_server(root).await);
    let entries = build_dir_listing(&sftp, Path::new("/")).await.unwrap();
    drop(sftp);

    let by_name: HashMap<&str, &FileEntry> = entries.iter().map(|e| (e.name.as_str(), e)).collect();

    // Parent navigation entry is always first.
    assert_eq!(entries[0].name, "..");
    assert!(by_name[".."].is_dir);
    assert!(!by_name[".."].is_symlink);

    // A link to a directory must be enterable: `is_dir` follows the link.
    let link_dir = by_name["link_dir"];
    assert!(link_dir.is_symlink);
    assert!(link_dir.is_dir);
    assert!(link_dir.size.is_none());
    assert!(link_dir.attributes.starts_with('l'));

    // A link to a file stays a file.
    let link_file = by_name["link_file"];
    assert!(link_file.is_symlink);
    assert!(!link_file.is_dir);

    // A broken link keeps `is_dir == false` and stays openable as a file.
    let link_broken = by_name["link_broken"];
    assert!(link_broken.is_symlink);
    assert!(!link_broken.is_dir);

    // A real directory and a plain file are unchanged by the fix.
    assert!(by_name["target"].is_dir);
    assert!(!by_name["target"].is_symlink);
    assert!(!by_name["plain.txt"].is_dir);
    assert!(by_name["plain.txt"].size.is_some());
}
