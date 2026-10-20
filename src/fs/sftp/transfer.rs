//! Directory listing and file-transfer helpers for the SFTP backend.
//!
//! The `FileSystemProvider` methods on [`SftpFs`] delegate here so the
//! provider impl stays a thin translation layer.

use super::SftpFs;
use crate::fs::provider::{FileSystemProvider, TaskProgressContext};
use crate::fs::utils::{
    FileEntry, build_du_command, format_sftp_permissions, is_dot_or_dotdot, normalize_sftp_path,
};
use anyhow::{Result, anyhow};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Builds the listing for `path`, resolving each symbolic link's target with a
/// STAT so linked directories are reported with `is_dir` and stay enterable
/// (mirrors the local `list_dir` behavior, which follows links too).
///
/// Link targets are resolved concurrently: listing a link-heavy remote
/// directory (e.g. `/usr/lib`, `/etc`) must not stall on sequential network
/// round trips. `SftpSession` is `Send + Sync`, so each target is looked up
/// with `metadata` (an SFTP STAT, which *follows* the link). A broken link, or
/// one removed mid-listing, keeps `is_dir == false` and stays a file, matching
/// local behavior.
pub(super) async fn build_dir_listing(
    sftp: &Arc<SftpSession>,
    path: &Path,
) -> Result<Vec<FileEntry>> {
    let path_str = normalize_sftp_path(path);
    let entries = sftp
        .read_dir(&path_str)
        .await
        .map_err(|e| anyhow!("Failed to read directory: {e}"))?;

    let mut names = Vec::new();
    let mut metas = Vec::new();
    let mut link_targets = Vec::new();

    for entry in entries {
        let name = entry.file_name();
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let meta = entry.metadata();
        let is_symlink = meta
            .permissions
            .is_some_and(|p| (p & 0o170_000) == 0o120_000);
        if is_symlink {
            // `base` drops any trailing `/` so root `/` and nested paths both
            // work; the child path is resolved with a follow-stat.
            let base = path_str.trim_end_matches('/');
            link_targets.push((name.clone(), format!("{base}/{name}")));
        }
        names.push(name);
        metas.push(meta);
    }

    // Resolve every symlink's target concurrently.
    let mut tasks = tokio::task::JoinSet::new();
    for (name, child) in link_targets {
        let sftp = Arc::clone(sftp);
        tasks.spawn(async move {
            let is_dir = sftp.metadata(&child).await.is_ok_and(|m| m.is_dir());
            (name, is_dir)
        });
    }
    let mut link_is_dir: HashMap<String, bool> = HashMap::new();
    while let Some(Ok((name, is_dir))) = tasks.join_next().await {
        link_is_dir.insert(name, is_dir);
    }

    let mut result = Vec::with_capacity(names.len() + 1);
    // prepend ".." for parent navigation
    result.push(FileEntry {
        name: "..".to_string(),
        is_dir: true,
        is_symlink: false,
        size: None,
        modified: None,
        attributes: String::new(),
        selected: false,
    });

    for (name, meta) in names.iter().zip(&metas) {
        let is_symlink = meta
            .permissions
            .is_some_and(|p| (p & 0o170_000) == 0o120_000);
        // `is_dir` follows the link for symlinks (resolved above with a
        // follow-stat); everything else keeps the READDIR type bits.
        let is_dir = if is_symlink {
            link_is_dir.get(name).copied().unwrap_or(false)
        } else {
            meta.is_dir()
        };
        let size = if is_dir { None } else { meta.size };
        let modified = meta
            .mtime
            .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(t)));
        result.push(FileEntry {
            name: name.clone(),
            is_dir,
            is_symlink,
            size,
            modified,
            attributes: format_sftp_permissions(meta.permissions.unwrap_or(0)),
            selected: false,
        });
    }
    Ok(result)
}

/// Recursively delete a remote directory via SFTP.
pub(super) async fn delete_recursive(sftp: &SftpSession, path: &str) -> Result<()> {
    let entries = sftp
        .read_dir(path)
        .await
        .map_err(|e| anyhow!("readdir {path}: {e}"))?;
    for entry in entries {
        let name = entry.file_name().clone();
        if is_dot_or_dotdot(&name) {
            continue;
        }
        let child = format!("{}/{}", path.trim_end_matches('/'), name);
        if entry.metadata().is_dir() {
            Box::pin(delete_recursive(sftp, &child)).await?;
        } else {
            sftp.remove_file(child)
                .await
                .map_err(|e| anyhow!("unlink: {e}"))?;
        }
    }
    sftp.remove_dir(path)
        .await
        .map_err(|e| anyhow!("rmdir {path}: {e}"))
}

pub(super) async fn calc_dir_size(fs: &SftpFs, path: &Path) -> anyhow::Result<u64> {
    let path_str = normalize_sftp_path(path);
    let cmd = build_du_command(&path_str);

    let mut channel = fs
        .session
        .channel_open_session()
        .await
        .map_err(|e| anyhow!("Failed to open channel: {e}"))?;

    channel
        .exec(true, cmd)
        .await
        .map_err(|e| anyhow!("Failed to exec du: {e}"))?;

    let output = tokio::time::timeout(SftpFs::DU_TIMEOUT, async {
        let mut output = Vec::new();
        loop {
            match channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    output.extend_from_slice(&data);
                }
                Some(russh::ChannelMsg::ExitStatus { .. } | russh::ChannelMsg::Eof) | None => {
                    break;
                }
                _ => {}
            }
        }
        output
    })
    .await
    .map_err(|_| {
        anyhow!(
            "Size calculation timed out after {}s (remote filesystem unresponsive?)",
            SftpFs::DU_TIMEOUT.as_secs()
        )
    })?;

    let out = String::from_utf8_lossy(&output);
    if out.trim().is_empty() {
        return Err(anyhow!(
            "Remote `du` produced no output (command unavailable or connection lost)"
        ));
    }
    let size_str = out.split('\t').next().unwrap_or("0").trim();
    let size: u64 = size_str
        .parse()
        .map_err(|e| anyhow!("Failed to parse du output '{}': {e}", out.trim()))?;
    Ok(size)
}

pub(super) async fn copy_to_local(
    fs: &SftpFs,
    src: &Path,
    dest_fs: &dyn FileSystemProvider,
    dest: &Path,
    progress: &TaskProgressContext,
) -> Option<anyhow::Result<()>> {
    let src_str = normalize_sftp_path(src);
    let dest_path = dest.to_path_buf();

    if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Some(Ok(()));
    }

    let (total_size, perms, is_dir) = match fs.sftp.metadata(src_str.as_str()).await {
        Ok(m) => (
            m.size.unwrap_or(0),
            m.permissions.map(|p| p & 0o777),
            (m.permissions.unwrap_or(0) & 0o170_000) == 0o040_000,
        ),
        Err(e) => return Some(Err(anyhow!("Failed to stat: {e}"))),
    };

    if is_dir {
        return None;
    }

    if total_size == 0 {
        if let Err(e) = dest_fs.write_file(&dest_path, &[]).await {
            return Some(Err(anyhow!("{e}")));
        }
        if let Some(p) = perms {
            let _ = dest_fs.set_permissions(&dest_path, p).await;
        }
        return Some(Ok(()));
    }

    let mut src_file = match fs.sftp.open(src_str).await {
        Ok(f) => f,
        Err(e) => return Some(Err(anyhow!("Failed to open source: {e}"))),
    };

    let chunk_size = fs.capabilities().latency.chunk_size(total_size);
    let mut buf = vec![0u8; chunk_size];
    let mut offset = 0u64;
    // Task-wide bytes before this file, so the chunk offsets below report
    // against the task total rather than restarting per file.
    let base = progress
        .processed_bytes
        .load(std::sync::atomic::Ordering::Relaxed);

    loop {
        if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            // Cancellation is not a failure: an `Err` here would raise the
            // error dialog and offer a retry for a transfer the user just
            // stopped. Remove the partial destination first so no truncated
            // file is left behind.
            let _ = dest_fs.delete(&dest_path, false).await;
            return Some(Ok(()));
        }
        let n = match src_file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Some(Err(anyhow!("Read error: {e}"))),
        };
        let chunk = buf[..n].to_vec();
        if let Err(e) = dest_fs.write_file_at(&dest_path, offset, &chunk).await {
            return Some(Err(anyhow!("{e}")));
        }
        offset += n as u64;
        progress
            .processed_bytes
            .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        progress.report_file_bytes(base, offset, total_size);
    }

    if let Some(p) = perms {
        let _ = dest_fs.set_permissions(&dest_path, p).await;
    }
    Some(Ok(()))
}

pub(super) async fn copy_from_local(
    fs: &SftpFs,
    src_fs: &dyn FileSystemProvider,
    src: &Path,
    dest: &Path,
    progress: &TaskProgressContext,
) -> Option<anyhow::Result<()>> {
    let dest_str = normalize_sftp_path(dest);
    let src_path = src.to_path_buf();

    if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Some(Ok(()));
    }

    if src_fs.is_dir(&src_path).await {
        return None;
    }

    let total_size = match src_fs.get_size(&src_path).await {
        Ok(s) => s,
        Err(e) => return Some(Err(e)),
    };
    let src_perms = src_fs.get_permissions(&src_path).await;
    let chunk_size = fs.capabilities().latency.chunk_size(total_size);

    if total_size == 0 {
        return Some(
            fs.sftp
                .create(dest_str.as_str())
                .await
                .map(|_| ())
                .map_err(|e| anyhow!("Failed to create empty file: {e}")),
        );
    }

    let mut dest_file = match fs.sftp.create(dest_str.as_str()).await {
        Ok(f) => f,
        Err(e) => return Some(Err(anyhow!("Failed to create destination: {e}"))),
    };

    let mut offset = 0u64;
    // Task-wide bytes before this file (see `copy_to_local`).
    let base = progress
        .processed_bytes
        .load(std::sync::atomic::Ordering::Relaxed);
    loop {
        if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            // Cancellation is not a failure (see `copy_to_local`): drop the
            // half-written destination and let the caller wind down.
            let _ = fs.sftp.remove_file(dest_str.as_str()).await;
            return Some(Ok(()));
        }
        let len = std::cmp::min(
            chunk_size,
            usize::try_from(total_size - offset).unwrap_or(usize::MAX),
        );
        if len == 0 {
            break;
        }
        let chunk = match src_fs.read_file_at(&src_path, offset, len).await {
            Ok(d) => d,
            Err(e) => return Some(Err(e)),
        };
        if let Err(e) = dest_file.write_all(&chunk).await {
            return Some(Err(anyhow!("Write error: {e}")));
        }
        offset += chunk.len() as u64;
        progress
            .processed_bytes
            .fetch_add(chunk.len() as u64, std::sync::atomic::Ordering::Relaxed);
        progress.report_file_bytes(base, offset, total_size);
    }

    if let Some(mode) = src_perms {
        let _ = fs
            .sftp
            .set_metadata(
                dest_str,
                FileAttributes {
                    size: None,
                    uid: None,
                    gid: None,
                    user: None,
                    group: None,
                    permissions: Some(mode),
                    atime: None,
                    mtime: None,
                },
            )
            .await;
    }
    Some(Ok(()))
}

pub(super) async fn supports_copy_to_local(
    fs: &SftpFs,
    src: &Path,
    _dest_fs: &dyn FileSystemProvider,
) -> bool {
    // `copy_to_local` handles files only; directories are recursed into.
    !fs.is_dir(src).await
}
